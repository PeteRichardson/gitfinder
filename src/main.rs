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

#[derive(Parser)]
struct Args {
    /// Directory to start walking from (default: ".")
    #[arg(default_value = ".")]
    dir: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let root_dir = tokio::fs::canonicalize(&args.dir).await?;

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

    // Temporary CSV output (replaced in Task 9)
    println!("repository,oldest,newest,count");
    for p in &all {
        let fmt = |iso: &Option<String>| {
            iso.as_deref()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| {
                    let local: chrono::DateTime<chrono::Local> = dt.into();
                    local.format("%y-%m-%d").to_string()
                })
                .unwrap_or_default()
        };
        println!(
            "{},{},{},{}",
            p.path,
            fmt(&p.oldest_unpushed),
            fmt(&p.newest_unpushed),
            p.unpushed_count,
        );
    }

    Ok(())
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
