# lsproj Phase 3 — CLI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expand lsproj from a CSV-only git-repo reporter into a full project metadata tool with table/JSON/CSV/schema output, all-branches unpushed commit tracking, tokei LOC counting, `.repostatus` support, and a `mark` subcommand.

**Architecture:** Restructure `main.rs` + `lib.rs` into focused modules (metadata, filter, git\_info, loc, fs\_meta, repostatus, output). Async tokio traversal classifies directories as Skip/Project/Collection; project roots feed a sync `extract_metadata()` run in `spawn_blocking`. Results accumulate in a shared `Arc<Mutex<Vec<ProjectMetadata>>>`, then main sorts and renders in the chosen format.

**Tech Stack:** Rust edition 2024, tokio 1.52 (replaces async-std), git2 0.20, tokei 14, comfy-table 7.2, serde/serde\_json 1, serde-saphyr 0.0.28, clap 4.5.

## Global Constraints

- Rust edition 2024 — no `async_std` imports remain after Task 1
- `cargo test`, `cargo fmt --check`, and `cargo clippy --all-targets --all-features -- -D warnings` must pass after every task
- Run `cargo fmt` before every commit
- Binary name stays `lsproj` (package name already set in `Cargo.toml`)
- All date strings stored internally in ISO 8601 format (`%Y-%m-%dT%H:%M:%SZ`); CSV output reformats to `%y-%m-%d` for backward compat

---

## File Map

| File | Action | Responsibility |
|---|---|---|
| `Cargo.toml` | Modify | Remove async-std/async-lock; add tokio, serde, serde\_json, serde-saphyr, tokei, comfy-table |
| `src/lib.rs` | Modify | `Filter<Path>` trait, `simplified_repo_path` — switch from `async_std::path::Path` to `std::path::Path`; add `mod` declarations |
| `src/main.rs` | Modify | CLI args+subcommands, tokio `walk_dir`, main |
| `src/metadata.rs` | Create | `ProjectMetadata`, `LanguageStat`, `extract_metadata()` |
| `src/filter.rs` | Create | `EntryKind`, `SKIP_DIRS`, `classify_entry()` |
| `src/git_info.rs` | Create | `GitInfo`, `extract_git_info()` — all-branches unpushed logic |
| `src/loc.rs` | Create | `LocInfo`, `extract_loc()` — tokei LOC + language detection |
| `src/fs_meta.rs` | Create | `FsInfo`, `extract_fs_info()` — has\_readme/tests/ci/license |
| `src/repostatus.rs` | Create | `RepoStatus`, `read_repostatus()`, `write_repostatus()` |
| `src/output.rs` | Create | `print_table()`, `print_json()`, `print_csv()`, `print_schema()` |
| `tests/cli.rs` | Modify | Update CSV tests to pass `--csv`; add new format/filter/mark tests |

---

### Task 1: Cargo.toml cleanup and tokio migration

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/lib.rs`
- Modify: `src/main.rs`

**Interfaces:**
- Produces: compiling binary with tokio replacing async-std; existing CSV output still works; all tests pass

- [ ] **Step 1: Update Cargo.toml** — remove `async-std` and `async-lock`, keep new deps that were added

Replace the full `[dependencies]` block in `Cargo.toml`:
```toml
[dependencies]
anyhow = "1.0.98"
chrono = "0.4.41"
clap = { version = "4.5.38", features = ["derive"] }
comfy-table = "7.2.2"
git2 = "0.20.2"
serde = { version = "1.0.228", features = ["derive"] }
serde-saphyr = "0.0.28"
serde_json = "1.0.150"
tokei = "14.0.0"
tokio = { version = "1.52.3", features = ["full"] }
```

- [ ] **Step 2: Migrate `src/lib.rs` to `std::path::Path`**

Replace `use async_std::path::Path;` with `use std::path::Path;`.
Update the doctest comment on `simplified_repo_path`:
```rust
/// use std::path::Path;
```
No other changes needed — `Path::file_name()`, `strip_prefix()`, `display()` all work identically on `std::path::Path`.

- [ ] **Step 3: Migrate `src/main.rs` to tokio**

Replace the full contents of `src/main.rs`:
```rust
use std::collections::HashSet;
use std::future::Future;
use std::path::{Path, PathBuf};
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

        let filter_dir =
            AddToGithub::new(&["target", ".build", "node_modules", "vendor", ".git"]);

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
```

- [ ] **Step 4: Verify build and tests pass**

```bash
cargo test
```
Expected: all 9 tests pass (5 integration + 3 lib unit + 1 doctest).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml src/lib.rs src/main.rs
git commit -m "chore: migrate from async-std to tokio"
```

---

### Task 2: Module scaffolding and ProjectMetadata struct

**Files:**
- Modify: `src/lib.rs`
- Create: `src/metadata.rs`
- Create: `src/filter.rs` (stub)
- Create: `src/git_info.rs` (stub)
- Create: `src/loc.rs` (stub)
- Create: `src/fs_meta.rs` (stub)
- Create: `src/repostatus.rs` (stub)
- Create: `src/output.rs` (stub)

**Interfaces:**
- Produces: `ProjectMetadata` and `LanguageStat` types accessible as `lsproj::metadata::{ProjectMetadata, LanguageStat}`

- [ ] **Step 1: Write the failing test**

Add to `src/metadata.rs` (create the file):
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct LanguageStat {
    pub name: String,
    pub code: u64,
    pub comments: u64,
    pub blanks: u64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ProjectMetadata {
    pub path: String,
    pub name: String,
    pub is_git: bool,
    pub is_worktree: bool,
    pub has_remote: bool,
    pub origin_url: Option<String>,
    pub is_on_github: bool,
    pub unpushed_count: u32,
    pub oldest_unpushed: Option<String>,
    pub newest_unpushed: Option<String>,
    pub branches_with_unpushed: Vec<String>,
    pub total_commits: u32,
    pub primary_language: Option<String>,
    pub languages: Vec<LanguageStat>,
    pub has_readme: bool,
    pub has_tests: bool,
    pub has_ci: bool,
    pub has_license: bool,
    pub last_modified: Option<String>,
    pub repostatus_state: String,
    pub repostatus_age_days: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_serializes_to_json() {
        let m = ProjectMetadata {
            path: "foo/bar".to_string(),
            name: "bar".to_string(),
            is_git: true,
            repostatus_state: "unreviewed".to_string(),
            total_commits: 5,
            ..Default::default()
        };
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"path\":\"foo/bar\""));
        assert!(json.contains("\"total_commits\":5"));
        assert!(json.contains("\"repostatus_state\":\"unreviewed\""));
    }
}
```

- [ ] **Step 2: Create stub modules**

Create `src/filter.rs`:
```rust
pub enum EntryKind { Skip, Project, Collection }
```

Create `src/git_info.rs`:
```rust
pub struct GitInfo {
    pub is_git: bool,
    pub is_worktree: bool,
    pub has_remote: bool,
    pub origin_url: Option<String>,
    pub is_on_github: bool,
    pub unpushed_count: u32,
    pub oldest_unpushed: Option<String>,
    pub newest_unpushed: Option<String>,
    pub branches_with_unpushed: Vec<String>,
    pub total_commits: u32,
    pub last_modified: Option<String>,
}

impl Default for GitInfo {
    fn default() -> Self {
        GitInfo {
            is_git: false,
            is_worktree: false,
            has_remote: false,
            origin_url: None,
            is_on_github: false,
            unpushed_count: 0,
            oldest_unpushed: None,
            newest_unpushed: None,
            branches_with_unpushed: Vec::new(),
            total_commits: 0,
            last_modified: None,
        }
    }
}
```

Create `src/loc.rs`:
```rust
use crate::metadata::LanguageStat;
pub struct LocInfo {
    pub languages: Vec<LanguageStat>,
    pub primary_language: Option<String>,
}
```

Create `src/fs_meta.rs`:
```rust
pub struct FsInfo {
    pub has_readme: bool,
    pub has_tests: bool,
    pub has_ci: bool,
    pub has_license: bool,
}
```

Create `src/repostatus.rs`:
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct RepoStatus {
    pub state: Option<String>,
    pub reason: Option<String>,
    pub effort: Option<String>,
    pub reviewed: Option<String>,
    pub notes: Option<String>,
}
```

Create `src/output.rs`:
```rust
use crate::metadata::ProjectMetadata;

pub fn print_table(_projects: &[ProjectMetadata]) { todo!() }
pub fn print_json(_projects: &[ProjectMetadata]) { todo!() }
pub fn print_csv(_projects: &[ProjectMetadata]) { todo!() }
pub fn print_schema() { todo!() }
```

- [ ] **Step 3: Add module declarations to `src/lib.rs`**

Add at the top of `src/lib.rs` (after existing `use` statements):
```rust
pub mod filter;
pub mod fs_meta;
pub mod git_info;
pub mod loc;
pub mod metadata;
pub mod output;
pub mod repostatus;
```

- [ ] **Step 4: Run the failing test to verify it fails for the right reason**

```bash
cargo test test_metadata_serializes_to_json
```
Expected: FAIL (module not yet connected properly or test runs but needs serde_json in scope — adjust if needed).

Actually this test should pass immediately since the types are defined. Run it to confirm PASS.

- [ ] **Step 5: Verify build**

```bash
cargo build
```
Expected: clean build (stub `todo!()` calls are fine at compile time).

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/metadata.rs src/filter.rs src/git_info.rs src/loc.rs src/fs_meta.rs src/repostatus.rs src/output.rs
git commit -m "feat: add module scaffolding and ProjectMetadata struct"
```

---

### Task 3: ProjectFilter, traversal classification, and cycle detection

**Files:**
- Modify: `src/filter.rs`
- Modify: `src/main.rs`
- Modify: `tests/cli.rs`

**Interfaces:**
- Consumes: `std::path::Path`
- Produces: `classify_entry(&Path) -> EntryKind` — replaces `AddToGithub::filter()` in `walk_dir`

- [ ] **Step 1: Write failing tests**

Add to `tests/cli.rs`:
```rust
#[test]
fn test_skips_worktree_directory() {
    let root = TempDir::new().unwrap();
    // Create a worktree-like dir: .git is a FILE not a dir
    let worktree = root.path().join("myworktree");
    std::fs::create_dir(&worktree).unwrap();
    // Create a real repo so the worktree has something to be "for"
    let real_repo = root.path().join("real_repo");
    std::fs::create_dir(&real_repo).unwrap();
    init_repo_with_commits(&real_repo, &[1_700_000_000]);
    // Simulate worktree: .git is a file
    std::fs::write(worktree.join(".git"), "gitdir: ../real_repo/.git\n").unwrap();
    // Put a non-hidden file in worktree so it would be a "project" if not for worktree check
    std::fs::write(worktree.join("main.rs"), "fn main() {}").unwrap();

    let stdout = run_lsproj(root.path());
    assert!(
        !stdout.contains("myworktree"),
        "worktree directory should be skipped, got:\n{}",
        stdout
    );
}

#[test]
fn test_collection_root_is_descended() {
    // A directory with only subdirs (no files) is a collection root — lsproj descends into it
    let root = TempDir::new().unwrap();
    let collection = root.path().join("mycollection");
    let repo_dir = collection.join("myrepo");
    std::fs::create_dir_all(&repo_dir).unwrap();
    init_repo_with_commits(&repo_dir, &[1_700_000_000]);

    // mycollection has only subdirs — it's a collection root
    // myrepo has files (post-init git has .git dir; after our changes, a dir with only
    // .git counts as collection, so add a real file)
    std::fs::write(repo_dir.join("main.rs"), "fn main() {}").unwrap();

    let stdout = run_lsproj(root.path());
    // myrepo should appear in output (reached by descending into mycollection)
    assert!(
        stdout.contains("myrepo"),
        "repo inside collection root should be found, got:\n{}",
        stdout
    );
}
```

Also add a `run_lsproj` helper (replace the old `run_gitfinder`):
```rust
fn run_lsproj(dir: &Path) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_lsproj"))
        .arg(dir)
        .output()
        .expect("run lsproj");
    String::from_utf8(output.stdout).expect("utf8 stdout")
}
```

- [ ] **Step 2: Run failing tests**

```bash
cargo test test_skips_worktree_directory test_collection_root_is_descended -- --nocapture
```
Expected: both FAIL (classification logic not implemented yet).

- [ ] **Step 3: Implement `src/filter.rs`**

Replace the contents of `src/filter.rs`:
```rust
use std::path::Path;

pub enum EntryKind {
    Skip,
    Project,
    Collection,
}

const SKIP_DIRS: &[&str] = &["target", ".build", "node_modules", "vendor", ".git", ".cache"];

/// Classify a directory entry for traversal.
///
/// Returns:
/// - `Skip` if the directory should be ignored entirely (known build/dep dirs, or a git worktree)
/// - `Project` if the directory is a project root (contains non-hidden files)
/// - `Collection` if the directory contains only subdirectories (descend into it)
pub fn classify_entry(path: &Path) -> EntryKind {
    // Skip known noise directories
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if SKIP_DIRS.contains(&name) {
            return EntryKind::Skip;
        }
    }

    // Skip git worktrees: .git is a FILE (not a dir) in a linked worktree
    let git_path = path.join(".git");
    if git_path.exists() && git_path.is_file() {
        return EntryKind::Skip;
    }

    // Project root: has at least one non-hidden file
    let has_real_file = std::fs::read_dir(path)
        .map(|rd| {
            rd.flatten().any(|e| {
                e.file_type().map(|ft| ft.is_file()).unwrap_or(false)
                    && !e.file_name().to_string_lossy().starts_with('.')
            })
        })
        .unwrap_or(false);

    if has_real_file {
        EntryKind::Project
    } else {
        EntryKind::Collection
    }
}
```

- [ ] **Step 4: Update `walk_dir` in `src/main.rs` to use `classify_entry`**

Update the `walk_dir` function body. Replace the imports at the top of main.rs to add:
```rust
use lsproj::filter::{EntryKind, classify_entry};
```

Remove the `use lsproj::{AddToGithub, Filter, ...}` import (keep `simplified_repo_path` import for now).

Replace the inner loop in `walk_dir` (the `while let Some(entry)` block):
```rust
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
            let path_display = path.display().to_string();
            let new_task = task::spawn(async move {
                if let Err(e) = print_git_repo_info(path_clone, root_clone).await {
                    eprintln!("Error reading repo {path_display}: {e:?}");
                }
            });
            tasks.lock().unwrap().push(new_task);
        }
        EntryKind::Collection => {
            let root_clone = root.clone();
            let tasks_clone = tasks.clone();
            let semaphore_clone = semaphore.clone();
            let seen_clone = seen_paths.clone();
            let path_clone = path.clone();
            let path_display = path.display().to_string();
            let new_task = task::spawn(async move {
                if let Err(e) = walk_dir(
                    path_clone,
                    root_clone,
                    tasks_clone,
                    semaphore_clone,
                    seen_clone,
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
```

Update `walk_dir`'s signature to add `seen_paths`:
```rust
fn walk_dir(
    dir: PathBuf,
    root: PathBuf,
    tasks: Arc<Mutex<Vec<JoinHandle<()>>>>,
    semaphore: Arc<Semaphore>,
    seen_paths: Arc<Mutex<HashSet<PathBuf>>>,
) -> Pin<Box<dyn Future<Output = Result<()>> + Send>>
```

Update `main()` to create `seen_paths` and pass it:
```rust
let seen_paths: Arc<Mutex<HashSet<PathBuf>>> = Arc::new(Mutex::new(HashSet::new()));
// ...
let seen_clone = seen_paths.clone();
let initial_task = task::spawn(async move {
    if let Err(e) = walk_dir(root_clone.clone(), root_clone, tasks_clone, semaphore, seen_clone).await {
        eprintln!("Error in root: {e:?}");
    }
});
```

- [ ] **Step 5: Run the new tests**

```bash
cargo test test_skips_worktree_directory test_collection_root_is_descended
```
Expected: both PASS.

- [ ] **Step 6: Run full test suite**

```bash
cargo test
```
Expected: all tests pass. Note: `test_csv_header_and_date_format` and `test_finds_repo_without_origin` may still pass since those repos have git dirs + real files (project roots), and repos without origin are now shown (classification changed: we report ALL project roots, not just repos-without-origin). If existing tests break because repos with origin now appear in output, that's expected — they will be fixed in Task 9 when CSV format tests are updated to use `--csv`.

- [ ] **Step 7: Commit**

```bash
git add src/filter.rs src/main.rs tests/cli.rs
git commit -m "feat: add project/collection/skip traversal classification with cycle detection"
```

---

### Task 4: Git metadata extraction

**Files:**
- Modify: `src/git_info.rs`
- Test: `src/git_info.rs` (inline `#[cfg(test)]`)

**Interfaces:**
- Consumes: `&std::path::Path`
- Produces: `GitInfo` struct with all git fields; never returns an error (failed git ops → `is_git: false`)

- [ ] **Step 1: Write failing tests**

Add to `src/git_info.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use git2::{Repository, Signature, Time};
    use tempfile::TempDir;

    fn make_repo(dir: &std::path::Path, commit_times: &[i64]) -> Repository {
        let repo = Repository::init(dir).unwrap();
        let tree_oid = repo.treebuilder(None).unwrap().write().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let mut parent_oid: Option<git2::Oid> = None;
        for &secs in commit_times {
            let sig = Signature::new("T", "t@t.com", &Time::new(secs, 0)).unwrap();
            let parents: Vec<git2::Oid> = parent_oid.into_iter().collect();
            let parent_commits: Vec<git2::Commit> =
                parents.iter().map(|&o| repo.find_commit(o).unwrap()).collect();
            let parent_refs: Vec<&git2::Commit> = parent_commits.iter().collect();
            let oid = repo
                .commit(Some("refs/heads/main"), &sig, &sig, "test", &tree, &parent_refs)
                .unwrap();
            parent_oid = Some(oid);
        }
        repo
    }

    #[test]
    fn test_no_git_repo() {
        let tmp = TempDir::new().unwrap();
        let info = extract_git_info(tmp.path());
        assert!(!info.is_git);
        assert_eq!(info.total_commits, 0);
        assert_eq!(info.unpushed_count, 0);
    }

    #[test]
    fn test_git_repo_no_remote() {
        let tmp = TempDir::new().unwrap();
        make_repo(tmp.path(), &[1_700_000_000, 1_700_100_000]);
        let info = extract_git_info(tmp.path());
        assert!(info.is_git);
        assert!(!info.has_remote);
        assert!(!info.is_on_github);
        assert_eq!(info.total_commits, 2);
        assert_eq!(info.unpushed_count, 2); // no remote → all commits are unpushed
        assert!(info.oldest_unpushed.is_some());
        assert!(info.newest_unpushed.is_some());
        assert!(info.branches_with_unpushed.contains(&"main".to_string()));
    }

    #[test]
    fn test_git_repo_with_origin() {
        let tmp = TempDir::new().unwrap();
        let repo = make_repo(tmp.path(), &[1_700_000_000]);
        repo.remote("origin", "git@github.com:user/myrepo.git").unwrap();
        let info = extract_git_info(tmp.path());
        assert!(info.is_git);
        assert!(info.has_remote);
        assert!(info.is_on_github);
        assert_eq!(info.origin_url, Some("git@github.com:user/myrepo.git".to_string()));
        // No remote tracking refs → all local commits still count as unpushed
        // (in practice you'd need to fetch and have remote refs, but local-only remote is fine)
    }
}
```

- [ ] **Step 2: Run failing tests**

```bash
cargo test test_no_git_repo test_git_repo_no_remote test_git_repo_with_origin
```
Expected: FAIL (functions not implemented).

- [ ] **Step 3: Implement `src/git_info.rs`**

Replace full contents:
```rust
use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use git2::{BranchType, Repository};

pub struct GitInfo {
    pub is_git: bool,
    pub is_worktree: bool,
    pub has_remote: bool,
    pub origin_url: Option<String>,
    pub is_on_github: bool,
    pub unpushed_count: u32,
    pub oldest_unpushed: Option<String>,
    pub newest_unpushed: Option<String>,
    pub branches_with_unpushed: Vec<String>,
    pub total_commits: u32,
    pub last_modified: Option<String>,
}

impl Default for GitInfo {
    fn default() -> Self {
        GitInfo {
            is_git: false,
            is_worktree: false,
            has_remote: false,
            origin_url: None,
            is_on_github: false,
            unpushed_count: 0,
            oldest_unpushed: None,
            newest_unpushed: None,
            branches_with_unpushed: Vec::new(),
            total_commits: 0,
            last_modified: None,
        }
    }
}

pub fn extract_git_info(path: &Path) -> GitInfo {
    inner(path).unwrap_or_default()
}

fn to_iso8601(secs: i64) -> String {
    let dt = DateTime::<Utc>::from(UNIX_EPOCH + Duration::from_secs(secs.unsigned_abs()));
    dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn inner(path: &Path) -> anyhow::Result<GitInfo> {
    let repo = Repository::open(path)?;

    // origin URL
    let origin_url = repo
        .find_remote("origin")
        .ok()
        .and_then(|r| r.url().map(|s| s.to_string()));
    let has_remote = repo.remotes()?.len() > 0;
    let is_on_github = origin_url
        .as_deref()
        .map(|u| u.contains("github.com"))
        .unwrap_or(false);

    // All remote ref tips (for hiding in revwalk)
    let remote_oids: HashSet<git2::Oid> = repo
        .references()?
        .flatten()
        .filter(|r| r.name().map(|n| n.starts_with("refs/remotes/")).unwrap_or(false))
        .filter_map(|r| r.target())
        .collect();

    // Total commits: walk from all local branch tips
    let mut total_revwalk = repo.revwalk()?;
    for branch_result in repo.branches(Some(BranchType::Local))? {
        if let Ok((branch, _)) = branch_result {
            if let Some(oid) = branch.get().target() {
                let _ = total_revwalk.push(oid);
            }
        }
    }
    let total_commits = total_revwalk.count() as u32;

    // Unpushed commits: per branch, hide all remote refs
    let mut branches_with_unpushed: Vec<String> = Vec::new();
    let mut all_unpushed_secs: Vec<i64> = Vec::new();

    for branch_result in repo.branches(Some(BranchType::Local))? {
        let (branch, _) = branch_result?;
        let branch_name = branch.name()?.unwrap_or("<unnamed>").to_string();
        let tip = match branch.get().target() {
            Some(oid) => oid,
            None => continue,
        };

        let mut revwalk = repo.revwalk()?;
        revwalk.push(tip)?;
        for &remote_oid in &remote_oids {
            let _ = revwalk.hide(remote_oid);
        }

        let mut branch_secs: Vec<i64> = Vec::new();
        for oid_result in revwalk {
            let commit = repo.find_commit(oid_result?)?;
            branch_secs.push(commit.time().seconds());
        }

        if !branch_secs.is_empty() {
            branches_with_unpushed.push(branch_name);
            all_unpushed_secs.extend(branch_secs);
        }
    }

    let unpushed_count = all_unpushed_secs.len() as u32;
    let oldest_unpushed = all_unpushed_secs.iter().copied().min().map(to_iso8601);
    let newest_unpushed = all_unpushed_secs.iter().copied().max().map(to_iso8601);

    // last_modified: most recent mtime in git index
    let mut index = repo.index()?;
    let last_modified = index
        .iter()
        .map(|e| e.mtime.seconds() as i64)
        .max()
        .map(to_iso8601);

    Ok(GitInfo {
        is_git: true,
        is_worktree: false, // worktrees are skipped in traversal; never reported
        has_remote,
        origin_url,
        is_on_github,
        unpushed_count,
        oldest_unpushed,
        newest_unpushed,
        branches_with_unpushed,
        total_commits,
        last_modified,
    })
}

#[cfg(test)]
mod tests {
    // (tests defined in Step 1 go here)
}
```

Note: move the `#[cfg(test)]` block from Step 1 inside the file above.

- [ ] **Step 4: Run the tests**

```bash
cargo test test_no_git_repo test_git_repo_no_remote test_git_repo_with_origin
```
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add src/git_info.rs
git commit -m "feat: implement all-branches git metadata extraction"
```

---

### Task 5: LOC extraction with tokei

**Files:**
- Modify: `src/loc.rs`

**Interfaces:**
- Consumes: `&std::path::Path`
- Produces: `LocInfo { languages: Vec<LanguageStat>, primary_language: Option<String> }`

- [ ] **Step 1: Write failing test**

Add to `src/loc.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_loc_rust_file() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("main.rs"), "fn main() {\n    println!(\"hi\");\n}\n").unwrap();
        let info = extract_loc(tmp.path());
        // tokei should detect Rust
        assert!(
            info.primary_language.as_deref() == Some("Rust"),
            "expected Rust, got {:?}",
            info.primary_language
        );
        assert!(!info.languages.is_empty());
        let rust = info.languages.iter().find(|l| l.name == "Rust").unwrap();
        assert!(rust.code > 0);
    }

    #[test]
    fn test_loc_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let info = extract_loc(tmp.path());
        assert!(info.languages.is_empty());
        assert!(info.primary_language.is_none());
    }
}
```

- [ ] **Step 2: Run failing tests**

```bash
cargo test test_loc_rust_file test_loc_empty_dir
```
Expected: FAIL (function not implemented).

- [ ] **Step 3: Implement `src/loc.rs`**

Replace full contents:
```rust
use std::path::Path;
use tokei::{Config, Languages};

use crate::metadata::LanguageStat;

pub struct LocInfo {
    pub languages: Vec<LanguageStat>,
    pub primary_language: Option<String>,
}

pub fn extract_loc(path: &Path) -> LocInfo {
    let config = Config::default();
    let mut languages = Languages::new();
    languages.get_statistics(&[path], &[], &config);

    let mut stats: Vec<LanguageStat> = languages
        .iter()
        .filter(|(_, lang)| lang.code > 0 || lang.comments > 0 || lang.blanks > 0)
        .map(|(lang_type, lang)| LanguageStat {
            name: lang_type.to_string(),
            code: lang.code as u64,
            comments: lang.comments as u64,
            blanks: lang.blanks as u64,
        })
        .collect();

    stats.sort_by(|a, b| b.code.cmp(&a.code));

    let primary_language = stats.first().map(|s| s.name.clone());

    LocInfo {
        languages: stats,
        primary_language,
    }
}

#[cfg(test)]
mod tests {
    // tests from Step 1 go here
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test test_loc_rust_file test_loc_empty_dir
```
Expected: both PASS.

- [ ] **Step 5: Commit**

```bash
git add src/loc.rs
git commit -m "feat: add tokei LOC and language detection"
```

---

### Task 6: Filesystem metadata

**Files:**
- Modify: `src/fs_meta.rs`

**Interfaces:**
- Consumes: `&std::path::Path`
- Produces: `FsInfo { has_readme, has_tests, has_ci, has_license }`

- [ ] **Step 1: Write failing tests**

Add to `src/fs_meta.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_detects_readme() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("README.md"), "# hello").unwrap();
        let info = extract_fs_info(tmp.path());
        assert!(info.has_readme);
        assert!(!info.has_license);
    }

    #[test]
    fn test_detects_license() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("LICENSE"), "MIT").unwrap();
        let info = extract_fs_info(tmp.path());
        assert!(info.has_license);
    }

    #[test]
    fn test_detects_tests_dir() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("tests")).unwrap();
        let info = extract_fs_info(tmp.path());
        assert!(info.has_tests);
    }

    #[test]
    fn test_detects_github_ci() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".github").join("workflows")).unwrap();
        let info = extract_fs_info(tmp.path());
        assert!(info.has_ci);
    }

    #[test]
    fn test_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let info = extract_fs_info(tmp.path());
        assert!(!info.has_readme);
        assert!(!info.has_tests);
        assert!(!info.has_ci);
        assert!(!info.has_license);
    }
}
```

- [ ] **Step 2: Run failing tests**

```bash
cargo test test_detects_readme test_detects_license test_detects_tests_dir test_detects_github_ci test_empty_dir
```
Expected: FAIL.

- [ ] **Step 3: Implement `src/fs_meta.rs`**

Replace full contents:
```rust
use std::path::Path;

pub struct FsInfo {
    pub has_readme: bool,
    pub has_tests: bool,
    pub has_ci: bool,
    pub has_license: bool,
}

pub fn extract_fs_info(path: &Path) -> FsInfo {
    let names: Vec<String> = std::fs::read_dir(path)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().to_lowercase())
                .collect()
        })
        .unwrap_or_default();

    let has_readme = names.iter().any(|n| n.starts_with("readme"));
    let has_license = names.iter().any(|n| n.starts_with("license"));
    let has_tests = path.join("tests").is_dir()
        || path.join("test").is_dir()
        || names.iter().any(|n| n.ends_with("_test.rs") || n.ends_with("_spec.rb") || n == "test.py");
    let has_ci = path.join(".github").join("workflows").is_dir()
        || path.join(".travis.yml").is_file()
        || path.join(".circleci").is_dir()
        || path.join("Jenkinsfile").is_file()
        || path.join(".gitlab-ci.yml").is_file();

    FsInfo { has_readme, has_tests, has_ci, has_license }
}

#[cfg(test)]
mod tests {
    // tests from Step 1 go here
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test test_detects_readme test_detects_license test_detects_tests_dir test_detects_github_ci test_empty_dir
```
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add src/fs_meta.rs
git commit -m "feat: add filesystem metadata extraction"
```

---

### Task 7: Repostatus reading

**Files:**
- Modify: `src/repostatus.rs`

**Interfaces:**
- Consumes: `&std::path::Path`
- Produces: `Option<RepoStatus>` (None if `.repostatus` absent)

- [ ] **Step 1: Write failing test**

Add to `src/repostatus.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_reads_repostatus() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join(".repostatus"),
            "state: skip\nreason: trivial\nreviewed: 2026-01-15\n",
        )
        .unwrap();
        let rs = read_repostatus(tmp.path()).unwrap();
        assert_eq!(rs.state.as_deref(), Some("skip"));
        assert_eq!(rs.reason.as_deref(), Some("trivial"));
        assert_eq!(rs.reviewed.as_deref(), Some("2026-01-15"));
    }

    #[test]
    fn test_absent_repostatus_returns_none() {
        let tmp = TempDir::new().unwrap();
        assert!(read_repostatus(tmp.path()).is_none());
    }
}
```

- [ ] **Step 2: Run failing tests**

```bash
cargo test test_reads_repostatus test_absent_repostatus_returns_none
```
Expected: FAIL.

- [ ] **Step 3: Implement `src/repostatus.rs`**

Replace full contents:
```rust
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct RepoStatus {
    pub state: Option<String>,
    pub reason: Option<String>,
    pub effort: Option<String>,
    pub reviewed: Option<String>,
    pub notes: Option<String>,
}

pub fn read_repostatus(path: &Path) -> Option<RepoStatus> {
    let content = std::fs::read_to_string(path.join(".repostatus")).ok()?;
    serde_saphyr::from_str(&content).ok()
}

#[cfg(test)]
mod tests {
    // tests from Step 1 go here
}
```

Add to `src/lib.rs` at the top (after existing use statements):
```rust
extern crate serde_saphyr;
```

Wait — actually no `extern crate` needed in Rust 2018+. Just use it directly:
```rust
use serde_saphyr;
```

No, the crate is imported by name. In `src/repostatus.rs`, just call `serde_saphyr::from_str(...)` directly. Rust resolves the crate by the name in `Cargo.toml` (dash → underscore). No `extern crate` needed.

- [ ] **Step 4: Run tests**

```bash
cargo test test_reads_repostatus test_absent_repostatus_returns_none
```
Expected: both PASS.

- [ ] **Step 5: Commit**

```bash
git add src/repostatus.rs
git commit -m "feat: add .repostatus YAML reading"
```

---

### Task 8: Wire up metadata extraction and output collection

**Files:**
- Modify: `src/metadata.rs`
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: `extract_git_info`, `extract_loc`, `extract_fs_info`, `read_repostatus`, `simplified_repo_path`
- Produces: `extract_metadata(path: &Path, root: &Path) -> Result<ProjectMetadata>` in `src/metadata.rs`; `walk_dir` in main now collects into `Arc<Mutex<Vec<ProjectMetadata>>>` and prints CSV after traversal

- [ ] **Step 1: Write failing test**

Add to `src/metadata.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_extract_metadata_non_git_dir() {
        let root = TempDir::new().unwrap();
        let project = root.path().join("myproj");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(project.join("main.py"), "print('hello')\n").unwrap();

        let meta = extract_metadata(&project, root.path()).unwrap();
        assert_eq!(meta.name, "myproj");
        assert!(!meta.is_git);
        assert_eq!(meta.total_commits, 0);
        assert_eq!(meta.repostatus_state, "unreviewed");
        assert!(meta.primary_language.as_deref() == Some("Python"));
    }
}
```

- [ ] **Step 2: Run failing test**

```bash
cargo test test_extract_metadata_non_git_dir
```
Expected: FAIL (function not yet implemented).

- [ ] **Step 3: Implement `extract_metadata` in `src/metadata.rs`**

Add imports and function (append to file after the struct definitions):
```rust
use std::path::Path;
use crate::fs_meta::extract_fs_info;
use crate::git_info::extract_git_info;
use crate::loc::extract_loc;
use crate::repostatus::read_repostatus;
use lsproj::simplified_repo_path;  // wait — this is the lib crate itself

// Actually, since metadata.rs is inside the lsproj crate, use:
// use crate::simplified_repo_path;  — if we move simplified_repo_path to lib.rs pub fn
// or just inline the path logic here
```

Wait — `simplified_repo_path` is defined in `src/lib.rs`. Since `src/metadata.rs` is part of the same crate, reference it as `crate::simplified_repo_path`.

Add to `src/metadata.rs`:
```rust
use std::path::Path;

use chrono::NaiveDate;

use crate::fs_meta::extract_fs_info;
use crate::git_info::extract_git_info;
use crate::loc::extract_loc;
use crate::repostatus::read_repostatus;

pub fn extract_metadata(path: &Path, root: &Path) -> anyhow::Result<ProjectMetadata> {
    let git = extract_git_info(path);
    let loc = extract_loc(path);
    let fs = extract_fs_info(path);
    let status = read_repostatus(path);

    let display_path = crate::simplified_repo_path(path, root);
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();

    let (repostatus_state, repostatus_age_days) = match status {
        Some(rs) => {
            let state = rs.state.unwrap_or_else(|| "unreviewed".to_string());
            let age = rs.reviewed.as_deref().and_then(|d| {
                NaiveDate::parse_from_str(d, "%Y-%m-%d").ok().map(|reviewed| {
                    let today = chrono::Local::now().date_naive();
                    (today - reviewed).num_days().max(0) as u32
                })
            });
            (state, age)
        }
        None => ("unreviewed".to_string(), None),
    };

    Ok(ProjectMetadata {
        path: display_path,
        name,
        is_git: git.is_git,
        is_worktree: git.is_worktree,
        has_remote: git.has_remote,
        origin_url: git.origin_url,
        is_on_github: git.is_on_github,
        unpushed_count: git.unpushed_count,
        oldest_unpushed: git.oldest_unpushed,
        newest_unpushed: git.newest_unpushed,
        branches_with_unpushed: git.branches_with_unpushed,
        total_commits: git.total_commits,
        primary_language: loc.primary_language,
        languages: loc.languages,
        has_readme: fs.has_readme,
        has_tests: fs.has_tests,
        has_ci: fs.has_ci,
        has_license: fs.has_license,
        last_modified: git.last_modified,
        repostatus_state,
        repostatus_age_days,
    })
}
```

- [ ] **Step 4: Update `src/main.rs` to collect results**

Replace the contents of `src/main.rs` to wire in result collection and temporary CSV output:

```rust
use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use anyhow::{Context, Result};
use chrono::{DateTime, Local};
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
                .map(|dt| dt.format("%y-%m-%d").to_string())
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

            if let Ok(canonical) = std::fs::canonicalize(&path) {
                let mut seen = seen_paths.lock().unwrap();
                if !seen.insert(canonical) {
                    continue;
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
```

- [ ] **Step 5: Run test**

```bash
cargo test test_extract_metadata_non_git_dir
```
Expected: PASS.

- [ ] **Step 6: Run full test suite**

```bash
cargo test
```
Note: `test_finds_repo_without_origin` and `test_excludes_repo_with_origin` from `tests/cli.rs` may no longer match expected output because all project roots are now reported (not just repos without origin) and output format is different from before. These will be properly updated in Task 9.

- [ ] **Step 7: Commit**

```bash
git add src/metadata.rs src/main.rs
git commit -m "feat: wire metadata extraction pipeline into walk_dir"
```

---

### Task 9: Output formats and updated CLI flags

**Files:**
- Modify: `src/output.rs`
- Modify: `src/main.rs`
- Modify: `tests/cli.rs`

**Interfaces:**
- Consumes: `&[ProjectMetadata]`
- Produces: `print_table()`, `print_json()`, `print_csv()`, `print_schema()` in `src/output.rs`; `--json`, `--csv`, `--schema` flags in Args

- [ ] **Step 1: Write failing tests**

Add to `tests/cli.rs`:
```rust
fn run_lsproj_with_args(dir: &Path, extra_args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_lsproj"))
        .arg(dir)
        .args(extra_args)
        .output()
        .expect("run lsproj")
}

#[test]
fn test_json_output_is_array() {
    let root = TempDir::new().unwrap();
    let repo_dir = root.path().join("myrepo");
    std::fs::create_dir(&repo_dir).unwrap();
    std::fs::write(repo_dir.join("main.rs"), "fn main() {}").unwrap();
    init_repo_with_commits(&repo_dir, &[1_700_000_000]);

    let output = run_lsproj_with_args(root.path(), &["--json"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(json.is_array(), "expected JSON array, got: {stdout}");
    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["name"], "myrepo");
    assert_eq!(arr[0]["is_git"], true);
}

#[test]
fn test_table_output_default() {
    let root = TempDir::new().unwrap();
    let repo_dir = root.path().join("myrepo");
    std::fs::create_dir(&repo_dir).unwrap();
    std::fs::write(repo_dir.join("main.rs"), "fn main() {}").unwrap();
    init_repo_with_commits(&repo_dir, &[1_700_000_000]);

    let output = run_lsproj_with_args(root.path(), &[]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    // Table output should have header columns
    assert!(stdout.contains("PATH"), "expected table header, got: {stdout}");
    assert!(stdout.contains("STATUS"), "expected table header, got: {stdout}");
    assert!(stdout.contains("myrepo"), "expected myrepo in table, got: {stdout}");
}
```

Also add `serde_json` to `[dev-dependencies]` in `Cargo.toml`:
```toml
[dev-dependencies]
serde_json = "1"
tempfile = "3"
```

Update the existing CSV tests (they use `run_gitfinder` which tests the old default CSV output; switch them to use `--csv`):
```rust
fn run_gitfinder(dir: &Path) -> String {
    // Keep old name for backward compat with existing tests, but add --csv
    let output = Command::new(env!("CARGO_BIN_EXE_lsproj"))
        .arg(dir)
        .arg("--csv")
        .output()
        .expect("run lsproj");
    assert!(output.status.success(), "lsproj exited with failure: {:?}", output);
    String::from_utf8(output.stdout).expect("utf8 stdout")
}
```

- [ ] **Step 2: Run failing tests**

```bash
cargo test test_json_output_is_array test_table_output_default
```
Expected: FAIL.

- [ ] **Step 3: Implement `src/output.rs`**

Replace full contents:
```rust
use comfy_table::{Attribute, Cell, Table};
use serde_json;

use crate::metadata::ProjectMetadata;

pub fn print_table(projects: &[ProjectMetadata]) {
    let mut table = Table::new();
    table.set_header(vec!["PATH", "LANG", "LOC", "COMMITS", "UNPUSHED", "STATUS"]);
    for p in projects {
        let total_loc: u64 = p.languages.iter().map(|l| l.code).sum();
        table.add_row(vec![
            p.path.clone(),
            p.primary_language.clone().unwrap_or_default(),
            total_loc.to_string(),
            p.total_commits.to_string(),
            p.unpushed_count.to_string(),
            p.repostatus_state.clone(),
        ]);
    }
    println!("{table}");
}

pub fn print_json(projects: &[ProjectMetadata]) {
    match serde_json::to_string_pretty(projects) {
        Ok(json) => println!("{json}"),
        Err(e) => eprintln!("JSON serialization error: {e}"),
    }
}

pub fn print_csv(projects: &[ProjectMetadata]) {
    println!("repository,oldest,newest,count");
    for p in projects {
        let fmt = |iso: &Option<String>| {
            iso.as_deref()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.format("%y-%m-%d").to_string())
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
}

pub fn print_schema() {
    // Minimal JSON Schema for ProjectMetadata
    let schema = serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": "ProjectMetadata",
        "type": "object",
        "properties": {
            "path":                    { "type": "string" },
            "name":                    { "type": "string" },
            "is_git":                  { "type": "boolean" },
            "is_worktree":             { "type": "boolean" },
            "has_remote":              { "type": "boolean" },
            "origin_url":              { "type": ["string", "null"] },
            "is_on_github":            { "type": "boolean" },
            "unpushed_count":          { "type": "integer" },
            "oldest_unpushed":         { "type": ["string", "null"] },
            "newest_unpushed":         { "type": ["string", "null"] },
            "branches_with_unpushed":  { "type": "array", "items": { "type": "string" } },
            "total_commits":           { "type": "integer" },
            "primary_language":        { "type": ["string", "null"] },
            "languages": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "name":     { "type": "string" },
                        "code":     { "type": "integer" },
                        "comments": { "type": "integer" },
                        "blanks":   { "type": "integer" }
                    }
                }
            },
            "has_readme":              { "type": "boolean" },
            "has_tests":               { "type": "boolean" },
            "has_ci":                  { "type": "boolean" },
            "has_license":             { "type": "boolean" },
            "last_modified":           { "type": ["string", "null"] },
            "repostatus_state":        { "type": "string" },
            "repostatus_age_days":     { "type": ["integer", "null"] }
        }
    });
    println!("{}", serde_json::to_string_pretty(&schema).unwrap());
}
```

- [ ] **Step 4: Update `src/main.rs` Args and output dispatch**

Replace `Args` struct and add `SubCommand` in `src/main.rs`:
```rust
#[derive(Parser)]
#[command(name = "lsproj", about = "List local projects with metadata")]
struct Args {
    /// Directory to scan
    #[arg(default_value = ".")]
    dir: PathBuf,

    /// Output as JSON array
    #[arg(long)]
    json: bool,

    /// Output as CSV (backward-compatible format)
    #[arg(long)]
    csv: bool,

    /// Print JSON Schema for ProjectMetadata
    #[arg(long)]
    schema: bool,

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
```

Replace the output block in `main()` (after collecting `all`):
```rust
// Handle subcommands first
if let Some(SubCommand::Mark { path, state, reason }) = args.command {
    todo!("mark subcommand — implemented in Task 11");
}

// Output
match (args.schema, args.json, args.csv) {
    (true, _, _) => lsproj::output::print_schema(),
    (_, true, _) => lsproj::output::print_json(&all),
    (_, _, true) => lsproj::output::print_csv(&all),
    _ => lsproj::output::print_table(&all),
}
```

Remove the temporary CSV println block from Task 8.

Also update the `use` statement at top of main.rs to add:
```rust
use lsproj::output;
```

- [ ] **Step 5: Run tests**

```bash
cargo test
```
Expected: all tests pass including `test_json_output_is_array`, `test_table_output_default`, and the existing CSV tests (now using `--csv` flag).

- [ ] **Step 6: Commit**

```bash
git add src/output.rs src/main.rs tests/cli.rs Cargo.toml
git commit -m "feat: add table/json/csv/schema output formats"
```

---

### Task 10: Filtering

**Files:**
- Modify: `src/main.rs`
- Modify: `tests/cli.rs`

**Interfaces:**
- Consumes: `--filter <STATE>` flag (repeatable); `Vec<String>` filters applied after collection
- Produces: filtered `Vec<ProjectMetadata>` passed to output functions

- [ ] **Step 1: Write failing test**

Add to `tests/cli.rs`:
```rust
#[test]
fn test_filter_no_git() {
    let root = TempDir::new().unwrap();
    // Non-git project (has files but no .git)
    let non_git = root.path().join("myscript");
    std::fs::create_dir(&non_git).unwrap();
    std::fs::write(non_git.join("script.py"), "print('hello')").unwrap();
    // Git project
    let git_proj = root.path().join("myrepo");
    std::fs::create_dir(&git_proj).unwrap();
    std::fs::write(git_proj.join("main.rs"), "fn main() {}").unwrap();
    init_repo_with_commits(&git_proj, &[1_700_000_000]);

    let output = run_lsproj_with_args(root.path(), &["--filter", "no-git"]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("myscript"), "expected myscript in --filter no-git output");
    assert!(!stdout.contains("myrepo"), "myrepo should be excluded by --filter no-git");
}

#[test]
fn test_filter_unreviewed() {
    let root = TempDir::new().unwrap();
    let repo_dir = root.path().join("myrepo");
    std::fs::create_dir(&repo_dir).unwrap();
    std::fs::write(repo_dir.join("main.rs"), "fn main() {}").unwrap();
    init_repo_with_commits(&repo_dir, &[1_700_000_000]);
    // No .repostatus file — state is "unreviewed" by default

    let output = run_lsproj_with_args(root.path(), &["--filter", "unreviewed", "--json"]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let arr = json.as_array().unwrap();
    assert!(!arr.is_empty(), "unreviewed project should appear");
    assert_eq!(arr[0]["repostatus_state"], "unreviewed");
}
```

- [ ] **Step 2: Run failing tests**

```bash
cargo test test_filter_no_git test_filter_unreviewed
```
Expected: FAIL.

- [ ] **Step 3: Add `--filter` flag to Args and apply filtering in `main()`**

Add to the `Args` struct in `src/main.rs`:
```rust
/// Filter by repostatus state. Valid values: unreviewed, pending, skip, ready, posted, no-git.
/// Can be specified multiple times.
#[arg(long, value_name = "STATE")]
filter: Vec<String>,
```

Add a filter function and apply it before output in `main()`. Insert after `all.sort_by(...)`:
```rust
let all = apply_filters(all, &args.filter);
```

Add the function (outside `main`, before `walk_dir`):
```rust
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
```

- [ ] **Step 4: Run tests**

```bash
cargo test test_filter_no_git test_filter_unreviewed
```
Expected: both PASS.

- [ ] **Step 5: Run full suite**

```bash
cargo test
```
Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs tests/cli.rs
git commit -m "feat: add --filter flag for repostatus state and no-git filtering"
```

---

### Task 11: Mark subcommand and repostatus writing

**Files:**
- Modify: `src/repostatus.rs`
- Modify: `src/main.rs`
- Modify: `tests/cli.rs`

**Interfaces:**
- Consumes: `lsproj mark <path> <state> [reason]`
- Produces: written/updated `.repostatus` YAML file in the target directory

- [ ] **Step 1: Write failing tests**

Add to `tests/cli.rs`:
```rust
#[test]
fn test_mark_creates_repostatus() {
    let root = TempDir::new().unwrap();
    let proj = root.path().join("myproj");
    std::fs::create_dir(&proj).unwrap();
    std::fs::write(proj.join("main.rs"), "fn main() {}").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_lsproj"))
        .args(["mark", proj.to_str().unwrap(), "skip", "trivial"])
        .output()
        .expect("run lsproj mark");
    assert!(output.status.success(), "mark failed: {:?}", output);

    let content = std::fs::read_to_string(proj.join(".repostatus")).unwrap();
    assert!(content.contains("state: skip"));
    assert!(content.contains("reason: trivial"));
    assert!(content.contains("reviewed:"));
}

#[test]
fn test_mark_updates_existing_repostatus() {
    let root = TempDir::new().unwrap();
    let proj = root.path().join("myproj");
    std::fs::create_dir(&proj).unwrap();
    std::fs::write(proj.join("main.rs"), "fn main() {}").unwrap();
    // Pre-existing .repostatus with notes
    std::fs::write(
        proj.join(".repostatus"),
        "state: pending\nnotes: |\n  Keep this note.\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_lsproj"))
        .args(["mark", proj.to_str().unwrap(), "ready"])
        .output()
        .expect("run lsproj mark");
    assert!(output.status.success());

    let content = std::fs::read_to_string(proj.join(".repostatus")).unwrap();
    assert!(content.contains("state: ready"), "state should be updated");
    assert!(content.contains("Keep this note"), "notes should be preserved");
}
```

- [ ] **Step 2: Run failing tests**

```bash
cargo test test_mark_creates_repostatus test_mark_updates_existing_repostatus
```
Expected: FAIL.

- [ ] **Step 3: Implement `write_repostatus` in `src/repostatus.rs`**

Add to `src/repostatus.rs`:
```rust
use chrono::Local;

pub fn write_repostatus(
    path: &Path,
    state: &str,
    reason: Option<&str>,
) -> anyhow::Result<()> {
    let file_path = path.join(".repostatus");
    let today = Local::now().format("%Y-%m-%d").to_string();

    // Read existing file to preserve notes and other fields
    let mut existing = read_repostatus(path).unwrap_or_default();

    existing.state = Some(state.to_string());
    existing.reason = reason.map(|s| s.to_string());
    existing.reviewed = Some(today);

    let content = serde_saphyr::to_string(&existing)?;
    std::fs::write(&file_path, content)?;
    Ok(())
}
```

- [ ] **Step 4: Wire `mark` subcommand in `src/main.rs`**

Replace the `todo!()` in the SubCommand::Mark match arm:
```rust
if let Some(SubCommand::Mark { path, state, reason }) = args.command {
    let canonical = tokio::fs::canonicalize(&path).await
        .with_context(|| format!("Path not found: {}", path.display()))?;
    lsproj::repostatus::write_repostatus(
        &canonical,
        &state,
        reason.as_deref(),
    )?;
    println!("Marked {} as {state}", canonical.display());
    return Ok(());
}
```

Note: when `command` is Some(Mark), skip the directory walk entirely and return after writing.

Also add the import at top of main.rs:
```rust
// No new imports needed — lsproj::repostatus is already in scope via lib.rs pub mod
```

- [ ] **Step 5: Run tests**

```bash
cargo test test_mark_creates_repostatus test_mark_updates_existing_repostatus
```
Expected: both PASS.

- [ ] **Step 6: Run full suite and lint**

```bash
cargo test && cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings
```
Fix any warnings before proceeding.

- [ ] **Step 7: Commit**

```bash
git add src/repostatus.rs src/main.rs tests/cli.rs
git commit -m "feat: add mark subcommand for writing .repostatus files"
```

---

## Self-Review

**Spec coverage check:**

| Spec requirement | Task |
|---|---|
| Traversal: collection root detection | Task 3 |
| Traversal: skip dirs (target, node\_modules, etc.) | Task 3 |
| Traversal: worktree detection (.git is file) | Task 3 |
| Traversal: cycle detection | Task 3 |
| ProjectMetadata struct (all fields) | Task 2 |
| Git: origin\_url, is\_on\_github, has\_remote | Task 4 |
| Git: all-branches unpushed count/dates/branches | Task 4 |
| Git: total\_commits | Task 4 |
| Git: last\_modified from index | Task 4 |
| LOC: tokei per-language stats | Task 5 |
| LOC: primary\_language | Task 5 |
| FS: has\_readme/tests/ci/license | Task 6 |
| .repostatus reading (state, age\_days) | Task 7 |
| Wire up extract\_metadata() | Task 8 |
| Output: table (default) | Task 9 |
| Output: --json | Task 9 |
| Output: --csv (backward compat) | Task 9 |
| Output: --schema | Task 9 |
| --filter flag (state + no-git) | Task 10 |
| mark subcommand | Task 11 |
| mark: write .repostatus with state/reason/reviewed | Task 11 |
| mark: preserve existing notes | Task 11 |

**Potential issues to watch:**

1. **`Arc::try_unwrap` in main**: If any background task is still holding a clone of `results` when main calls `try_unwrap`, it will panic. The current loop-until-empty approach ensures all tasks complete before this call, so it is safe.

2. **serde-saphyr YAML serialization format**: `serde_saphyr::to_string` emits YAML. Verify the output for `RepoStatus` with optional fields is clean YAML (no `null:` lines for missing fields). If serde serializes `None` as `~` or `null`, add `#[serde(skip_serializing_if = "Option::is_none")]` to RepoStatus fields.

3. **clap subcommand + positional arg conflict**: The `dir` positional arg and `command` subcommand might conflict when the first arg is `mark`. Test `lsproj mark . skip` explicitly after Task 11. If clap fails to parse, restructure `dir` as `Option<PathBuf>` and default to `.` in main.

4. **tokei path argument**: `Languages::get_statistics(&[path], ...)` takes `&[&Path]` or similar. Verify exact type — it may need `&[path.to_path_buf()]` or `&[path]` wrapped in a slice.
