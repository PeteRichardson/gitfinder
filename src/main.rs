use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use clap::Parser;
use tokio::sync::Semaphore;
use tokio::task::{self, JoinHandle};

use lsproj::filter::{EntryKind, classify_entry};
use lsproj::metadata::{ProjectMetadata, extract_metadata};
use lsproj::output;

#[derive(Parser)]
#[command(name = "lsproj", about = "List local projects with metadata")]
struct Args {
    /// Directory to scan
    dir: Option<PathBuf>,

    /// Output as JSON array
    #[arg(long)]
    json: bool,

    /// Output as CSV (backward-compatible format)
    #[arg(long)]
    csv: bool,

    /// Print JSON Schema for ProjectMetadata
    #[arg(long)]
    schema: bool,

    /// Filter by repostatus state. Valid values: unreviewed, pending, skip, ready, posted, no-git.
    /// Can be specified multiple times.
    #[arg(long, value_name = "STATE")]
    filter: Vec<String>,

    #[command(subcommand)]
    command: Option<SubCommand>,
}

#[derive(clap::Subcommand)]
enum SubCommand {
    /// Mark a project directory with a repostatus state
    Mark {
        /// Project directory path
        path: PathBuf,
        /// State: pending | skip | ready | posted
        state: String,
        /// Optional reason
        reason: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    if args.schema {
        output::print_schema();
        return Ok(());
    }

    if let Some(SubCommand::Mark {
        path,
        state,
        reason,
    }) = args.command
    {
        let canonical = tokio::fs::canonicalize(&path)
            .await
            .with_context(|| format!("Path not found: {}", path.display()))?;
        lsproj::repostatus::write_repostatus(&canonical, &state, reason.as_deref())?;
        println!("Marked {} as {state}", canonical.display());
        return Ok(());
    }

    let scan_dir = args.dir.unwrap_or_else(|| PathBuf::from("."));
    let root_dir = tokio::fs::canonicalize(&scan_dir).await?;

    if let Ok(repo) = git2::Repository::discover(&root_dir) {
        if let Some(workdir) = repo.workdir() {
            let workdir = workdir.to_path_buf();
            let parent = workdir.parent().unwrap_or(&workdir).to_path_buf();
            let meta =
                task::spawn_blocking(move || extract_metadata(&workdir, &parent)).await??;
            let results = apply_filters(vec![meta], &args.filter);
            match (args.json, args.csv) {
                (true, _) => output::print_json(&results),
                (_, true) => output::print_csv(&results),
                _ => output::print_table(&results),
            }
            return Ok(());
        }
    }

    let tasks: Arc<Mutex<Vec<JoinHandle<()>>>> = Arc::new(Mutex::new(Vec::new()));
    let semaphore = Arc::new(Semaphore::new(100));
    let seen_paths: Arc<Mutex<HashSet<PathBuf>>> = Arc::new(Mutex::new(HashSet::new()));
    let results: Arc<Mutex<Vec<ProjectMetadata>>> = Arc::new(Mutex::new(Vec::new()));

    let tasks_clone = tasks.clone();
    let root_clone = root_dir.clone();
    let seen_clone = seen_paths.clone();
    let results_clone = results.clone();

    let initial_task = task::spawn(async move {
        if let Err(e) = walk_dir(
            root_clone.clone(),
            root_clone,
            tasks_clone,
            semaphore,
            seen_clone,
            results_clone,
        )
        .await
        {
            eprintln!("Error in root: {e:?}");
        }
    });
    tasks.lock().unwrap().push(initial_task);

    loop {
        let current_tasks = {
            let mut locked = tasks.lock().unwrap();
            if locked.is_empty() {
                break;
            }
            std::mem::take(&mut *locked)
        };
        for handle in current_tasks {
            let _ = handle.await;
        }
    }

    let mut all = Arc::try_unwrap(results)
        .expect("results arc still held")
        .into_inner()
        .unwrap();
    all.sort_by(|a, b| a.path.cmp(&b.path));
    let all = apply_filters(all, &args.filter);

    match (args.json, args.csv) {
        (true, _) => output::print_json(&all),
        (_, true) => output::print_csv(&all),
        _ => output::print_table(&all),
    }

    Ok(())
}

fn apply_filters(projects: Vec<ProjectMetadata>, filters: &[String]) -> Vec<ProjectMetadata> {
    if filters.is_empty() {
        return projects;
    }
    projects
        .into_iter()
        .filter(|p| {
            filters.iter().any(|f| match f.as_str() {
                "no-git" => !p.is_git,
                state => p.repostatus_state == state,
            })
        })
        .collect()
}

fn walk_dir(
    dir: PathBuf,
    root: PathBuf,
    tasks: Arc<Mutex<Vec<JoinHandle<()>>>>,
    semaphore: Arc<Semaphore>,
    seen_paths: Arc<Mutex<HashSet<PathBuf>>>,
    results: Arc<Mutex<Vec<ProjectMetadata>>>,
) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
    Box::pin(async move {
        let _permit = semaphore.acquire().await?;

        let mut read_dir = tokio::fs::read_dir(&dir)
            .await
            .with_context(|| format!("Failed to read directory: {}", dir.display()))?;

        while let Some(entry) = read_dir
            .next_entry()
            .await
            .with_context(|| format!("Failed to read entry in {}", dir.display()))?
        {
            let path = entry.path();
            let ft = entry
                .file_type()
                .await
                .with_context(|| format!("Failed to get file type for {}", path.display()))?;

            if !ft.is_dir() {
                continue;
            }

            // Check canonical path for cycle detection
            if let Ok(canonical) = std::fs::canonicalize(&path) {
                let mut seen = seen_paths.lock().unwrap();
                if !seen.insert(canonical) {
                    continue; // already visited via a symlink — skip
                }
            }

            match classify_entry(&path) {
                EntryKind::Skip => {}
                EntryKind::Project => {
                    let root_clone = root.clone();
                    let path_clone = path.clone();
                    let results_clone = results.clone();
                    let path_display = path.display().to_string();
                    let new_task = task::spawn(async move {
                        let result = task::spawn_blocking(move || {
                            extract_metadata(&path_clone, &root_clone)
                        })
                        .await;
                        match result {
                            Ok(Ok(meta)) => results_clone.lock().unwrap().push(meta),
                            Ok(Err(e)) => eprintln!("Error extracting {path_display}: {e:?}"),
                            Err(e) => eprintln!("Task panic for {path_display}: {e:?}"),
                        }
                    });
                    tasks.lock().unwrap().push(new_task);
                }
                EntryKind::Collection => {
                    let root_clone = root.clone();
                    let tasks_clone = tasks.clone();
                    let semaphore_clone = semaphore.clone();
                    let seen_clone = seen_paths.clone();
                    let results_clone = results.clone();
                    let path_clone = path.clone();
                    let path_display = path.display().to_string();
                    let new_task = task::spawn(async move {
                        if let Err(e) = walk_dir(
                            path_clone,
                            root_clone,
                            tasks_clone,
                            semaphore_clone,
                            seen_clone,
                            results_clone,
                        )
                        .await
                        {
                            eprintln!("Error in {path_display}: {e:?}");
                        }
                    });
                    tasks.lock().unwrap().push(new_task);
                }
            }
        }

        Ok(())
    })
}
