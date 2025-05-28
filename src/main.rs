use async_lock::Semaphore; // ✅ Correct, async-std-compatible Semaphore!
use async_std::fs;
use async_std::path::{Path, PathBuf};
use async_std::stream::StreamExt;
use async_std::task::{self, JoinHandle};
use gitfinder::{AddToGithub, Filter};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use clap::Parser;

/// Walk a directory tree asynchronously with bounded concurrency.
#[derive(Parser)]
struct Args {
    /// Directory to start walking from (default: ".")
    #[arg(short = 'd', long = "dir", default_value = ".")]
    dir: PathBuf,
}

#[async_std::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let root_dir = args.dir;

    let tasks: Arc<Mutex<Vec<JoinHandle<()>>>> = Arc::new(Mutex::new(Vec::new()));
    let current_count = Arc::new(Mutex::new(0));
    let max_count = Arc::new(Mutex::new(0));

    // Concurrency limiter to avoid "Too Many Open Files"
    let concurrency_limit = 100; // adjust based on your system
    let semaphore = Arc::new(Semaphore::new(concurrency_limit));

    let tasks_clone = tasks.clone();
    let current_clone = current_count.clone();
    let max_clone = max_count.clone();
    let semaphore_clone = semaphore.clone();

    let initial_task = task::spawn(async move {
        if let Err(e) = walk_dir(
            root_dir,
            tasks_clone,
            current_clone,
            max_clone,
            semaphore_clone,
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

fn print_repo_data(path: &Path) {
    println!("found a git repo: {}", path.display());
}

fn walk_dir(
    dir: PathBuf,
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
                if filter_dir.filter(&path.as_path()) {
                    print_repo_data(&path);
                } else {
                    let tasks_clone = tasks.clone();
                    let current_clone = current_count.clone();
                    let max_clone = max_count.clone();
                    let semaphore_clone = semaphore.clone();
                    let path_clone = path.clone();
                    let new_task = task::spawn(async move {
                        if let Err(e) = walk_dir(
                            path_clone,
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
