use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use clap::Parser;
use git2::Repository;
use tokio::sync::Semaphore;
use tokio::task::{self, JoinHandle};

use lsproj::{AddToGithub, Filter, simplified_repo_path};

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
    let tasks_clone = tasks.clone();
    let root_clone = root_dir.clone();

    println!("repository,oldest,newest,count");

    let initial_task = task::spawn(async move {
        if let Err(e) = walk_dir(root_clone.clone(), root_clone, tasks_clone, semaphore).await {
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

    Ok(())
}

async fn print_git_repo_info(repo_path: PathBuf, root_path: PathBuf) -> Result<()> {
    task::spawn_blocking(move || {
        let repo = Repository::open(&repo_path)?;

        let branch = repo
            .find_branch("main", git2::BranchType::Local)
            .or_else(|_| repo.find_branch("master", git2::BranchType::Local))?;

        let oid = branch
            .get()
            .target()
            .ok_or_else(|| anyhow::anyhow!("Invalid branch target"))?;

        let mut revwalk = repo.revwalk()?;
        revwalk.push(oid)?;

        let mut earliest: Option<git2::Time> = None;
        let mut latest: Option<git2::Time> = None;
        let mut count = 0;

        for commit_id in revwalk {
            let commit = repo.find_commit(commit_id?)?;
            let t = commit.time();
            if earliest.is_none() || t.seconds() < earliest.unwrap().seconds() {
                earliest = Some(t);
            }
            if latest.is_none() || t.seconds() > latest.unwrap().seconds() {
                latest = Some(t);
            }
            count += 1;
        }

        let fmt = |t: Option<git2::Time>| {
            t.map(|t| {
                let st = UNIX_EPOCH + Duration::from_secs(t.seconds().unsigned_abs());
                let dt: DateTime<Local> = DateTime::from(st);
                format!("{}", dt.format("%y-%m-%d"))
            })
            .unwrap_or_default()
        };

        println!(
            "{},{},{},{}",
            simplified_repo_path(&repo_path, &root_path),
            fmt(earliest),
            fmt(latest),
            count
        );

        anyhow::Ok(())
    })
    .await
    .context("spawn_blocking panicked")?
}

fn walk_dir(
    dir: PathBuf,
    root: PathBuf,
    tasks: Arc<Mutex<Vec<JoinHandle<()>>>>,
    semaphore: Arc<Semaphore>,
) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
    Box::pin(async move {
        let _permit = semaphore.acquire().await?;

        let filter_dir = AddToGithub::new(&["target", ".build", "node_modules", "vendor", ".git"]);

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

            if ft.is_dir() {
                if filter_dir.filter(&path) {
                    let root_clone = root.clone();
                    let path_clone = path.clone();
                    let new_task = task::spawn(async move {
                        if let Err(e) = print_git_repo_info(path_clone, root_clone).await {
                            eprintln!("Error reading repo {}: {e:?}", path.display());
                        }
                    });
                    tasks.lock().unwrap().push(new_task);
                } else {
                    let root_clone = root.clone();
                    let tasks_clone = tasks.clone();
                    let semaphore_clone = semaphore.clone();
                    let path_clone = path.clone();
                    let new_task = task::spawn(async move {
                        if let Err(e) =
                            walk_dir(path_clone, root_clone, tasks_clone, semaphore_clone).await
                        {
                            eprintln!("Error in {}: {e:?}", path.display());
                        }
                    });
                    tasks.lock().unwrap().push(new_task);
                }
            }
        }

        Ok(())
    })
}
