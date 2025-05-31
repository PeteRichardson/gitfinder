use async_lock::Semaphore; // ✅ Correct, async-std-compatible Semaphore!
use async_std::fs;
use async_std::path::{Path, PathBuf};
use async_std::stream::StreamExt;
use async_std::task::{self, JoinHandle};
use chrono::{DateTime, Local};
use git2::Repository;
use gitfinder::{AddToGithub, Filter, simplified_repo_path};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use anyhow::{Context, Result};
use clap::Parser;

/// Walk a directory tree asynchronously with bounded concurrency.
#[derive(Parser)]
struct Args {
    /// Directory to start walking from (default: ".")
    #[arg(default_value = ".")]
    dir: PathBuf,
}

#[async_std::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let root_dir = fs::canonicalize(args.dir).await?;

    let tasks: Arc<Mutex<Vec<JoinHandle<()>>>> = Arc::new(Mutex::new(Vec::new()));
    let current_count = Arc::new(Mutex::new(0));
    let max_count = Arc::new(Mutex::new(0));

    // Concurrency limiter to avoid "Too Many Open Files"
    let concurrency_limit = 100;
    let semaphore = Arc::new(Semaphore::new(concurrency_limit));
    let tasks_clone = tasks.clone();

    println!("repository,oldest,newest,count");

    let initial_task = task::spawn(async move {
        if let Err(e) = walk_dir(
            root_dir.clone(), // starting_dir = root_dir for initial walk_dir
            root_dir,
            tasks_clone,
            current_count.clone(),
            max_count.clone(),
            semaphore.clone(),
        )
        .await
        {
            eprintln!("Error in root: {:?}", e);
        }
    });
    tasks.lock().unwrap().push(initial_task);

    // Wait for all tasks to complete.
    loop {
        let current_tasks = {
            let mut locked = tasks.lock().unwrap();
            if locked.is_empty() {
                break;
            }
            std::mem::take(&mut *locked)
        };

        for handle in current_tasks {
            handle.await;
        }
    }

    // Report the peak concurrency seen.
    //let max_concurrent = *max_count.lock().unwrap();
    //println!("Max concurrent tasks: {}", max_concurrent);

    Ok(())
}

/// Asynchronously prints:
/// 1. number of commits on main
/// 2. earliest commit date
/// 3. latest commit date
pub async fn print_git_repo_info(repo_path: &Path, root_path: PathBuf) -> anyhow::Result<()> {
    // Convert async_std::Path to std::path::Path
    let std_path = repo_path.to_path_buf();

    async_std::task::spawn_blocking(move || {
        // Open the repository
        let repo = Repository::open(std_path)?;

        // Find the main branch (it might be "main" or "master")
        let branch = repo
            .find_branch("main", git2::BranchType::Local)
            .or_else(|_| repo.find_branch("master", git2::BranchType::Local))?;
        let oid = branch
            .get()
            .target()
            .ok_or_else(|| anyhow::anyhow!("Invalid branch target"))?;
        let mut revwalk = repo.revwalk()?;
        revwalk.push(oid)?;

        // Track commit info
        let mut earliest: Option<git2::Time> = None;
        let mut latest: Option<git2::Time> = None;
        let mut count = 0;

        for commit_id in revwalk {
            let commit_id = commit_id?;
            let commit = repo.find_commit(commit_id)?;

            let commit_time = commit.time();

            if earliest.is_none() || commit_time.seconds() < earliest.unwrap().seconds() {
                earliest = Some(commit_time);
            }
            if latest.is_none() || commit_time.seconds() > latest.unwrap().seconds() {
                latest = Some(commit_time);
            }

            count += 1;
        }

        let print_time = |time: Option<git2::Time>| {
            if let Some(t) = time {
                let system_time = UNIX_EPOCH + Duration::from_secs(t.seconds().unsigned_abs());
                let datetime: DateTime<Local> = DateTime::from(system_time);
                print!(",{}", datetime.format("%y-%m-%d"));
            } else {
                println!(",");
            }
        };

        print!("{}", simplified_repo_path(repo.path().into(), &root_path));
        print_time(earliest);
        print_time(latest);
        println!(",{}", count);

        anyhow::Ok(())
    })
    .await
}

fn walk_dir(
    dir: PathBuf,
    root: PathBuf,
    tasks: Arc<Mutex<Vec<JoinHandle<()>>>>,
    current_count: Arc<Mutex<usize>>,
    max_count: Arc<Mutex<usize>>,
    semaphore: Arc<Semaphore>,
) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
    Box::pin(async move {
        // 🚦 Acquire a permit to limit concurrency
        let _permit = semaphore.acquire().await;

        // Increment active count
        let _current = {
            let mut current_locked = current_count.lock().unwrap();
            *current_locked += 1;

            // Update max if needed
            let mut max_locked = max_count.lock().unwrap();
            if *current_locked > *max_locked {
                *max_locked = *current_locked;
            }

            *current_locked
        };
        //println!("Task started. Current active: {}", current);
        let filter_dir = AddToGithub::new::<&str>(&[]);

        let mut entries = fs::read_dir(&dir)
            .await
            .with_context(|| format!("Failed to read directory: {}", dir.display()))?;

        while let Some(entry) = entries
            .next()
            .await
            .transpose()
            .with_context(|| format!("Failed to read entry in {}", dir.display()))?
        {
            let path = entry.path();
            let file_type = entry
                .file_type()
                .await
                .with_context(|| format!("Failed to get file type for {}", path.display()))?;

            if file_type.is_dir() {
                let root_clone = root.clone();
                if filter_dir.filter(&path) {
                    print_git_repo_info(&path, root_clone).await?;
                } else {
                    let tasks_clone = tasks.clone();
                    let current_clone = current_count.clone();
                    let max_clone = max_count.clone();
                    let semaphore_clone = semaphore.clone();
                    let path_clone = path.clone();
                    let new_task = task::spawn(async move {
                        if let Err(e) = walk_dir(
                            path_clone,
                            root_clone,
                            tasks_clone,
                            current_clone,
                            max_clone,
                            semaphore_clone,
                        )
                        .await
                        {
                            eprintln!("Error in {}: {:?}", path.display(), e);
                        }
                    });
                    tasks.lock().unwrap().push(new_task);
                }
            }
        }

        // Decrement active count
        let _current = {
            let mut locked = current_count.lock().unwrap();
            *locked -= 1;
            *locked
        };

        Ok(())
    })
}
