# Git Repo Discovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When `lsproj` is invoked with a path that is at or inside a git repo root, detect the enclosing repo via `git2::Repository::discover` and report it as the single project instead of traversing into its subdirectories.

**Architecture:** Add a discovery check in `main()` after computing `root_dir` and before starting the async walk. If `git2::Repository::discover(&root_dir)` succeeds and returns a workdir, call `extract_metadata` on that workdir, run it through the same output path as the normal walk, and return early. No changes to `filter.rs`, `classify_entry`, or `walk_dir`.

**Tech Stack:** Rust, `git2` 0.20.2 (already in `Cargo.toml`), `tokio`, `tempfile` (dev)

## Global Constraints

- `git2` is already a dependency — no new entries in `Cargo.toml` needed
- All `git2` calls must run inside `task::spawn_blocking` (git2 is synchronous)
- Display path must use `simplified_repo_path(workdir, workdir.parent())` — same format as normal scan output
- `--json`, `--csv`, `--filter`, and `--schema` must all behave identically to a normal scan
- `--schema` and `mark` subcommand already short-circuit before `root_dir` is computed — discovery check goes after those, not before

---

## File Map

| File | Change |
|------|--------|
| `src/main.rs` | Add discovery check after `root_dir` is computed |
| `tests/cli.rs` | Add two new integration tests |

---

### Task 1: Integration tests for git repo discovery

**Files:**
- Modify: `tests/cli.rs`

**Interfaces:**
- Consumes: `run_lsproj_with_args(dir: &Path, extra_args: &[&str]) -> std::process::Output` (already defined in `tests/cli.rs:67`)
- Consumes: `init_repo_with_commits(path: &Path, commit_times: &[i64]) -> Repository` (already defined in `tests/cli.rs:13`)

- [ ] **Step 1: Add two failing tests to `tests/cli.rs`**

Append both tests to the end of `tests/cli.rs`:

```rust
#[test]
fn test_discover_from_repo_root() {
    // Scanning the repo root itself (not its parent) should report the repo.
    let root = TempDir::new().unwrap();
    let repo_dir = root.path().join("myrepo");
    std::fs::create_dir(&repo_dir).unwrap();
    std::fs::write(repo_dir.join("main.rs"), "fn main() {}").unwrap();
    init_repo_with_commits(&repo_dir, &[1_700_000_000]);

    let output = run_lsproj_with_args(&repo_dir, &["--json"]);
    assert!(output.status.success(), "lsproj failed: {:?}", output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 1, "expected exactly one project, got:\n{stdout}");
    assert_eq!(arr[0]["name"], "myrepo");
    assert_eq!(arr[0]["is_git"], true);
}

#[test]
fn test_discover_from_inside_repo() {
    // Scanning a subdirectory inside a repo should report the enclosing repo root.
    let root = TempDir::new().unwrap();
    let repo_dir = root.path().join("myrepo");
    let src_dir = repo_dir.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("main.rs"), "fn main() {}").unwrap();
    init_repo_with_commits(&repo_dir, &[1_700_000_000]);

    let output = run_lsproj_with_args(&src_dir, &["--json"]);
    assert!(output.status.success(), "lsproj failed: {:?}", output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 1, "expected exactly one project, got:\n{stdout}");
    assert_eq!(arr[0]["name"], "myrepo", "should report repo root, not src subdir");
    assert_eq!(arr[0]["is_git"], true);
}
```

- [ ] **Step 2: Run the new tests to confirm they fail**

```bash
cargo test test_discover_ 2>&1 | tail -20
```

Expected: both tests FAIL. `test_discover_from_repo_root` will likely produce empty JSON `[]` (no projects found) because `lsproj` currently walks into `myrepo`'s entries and finds nothing matching its heuristics. `test_discover_from_inside_repo` will similarly fail.

---

### Task 2: Add discovery check to `main()`

**Files:**
- Modify: `src/main.rs:79-84` (after `root_dir` is computed, before walk setup)

**Interfaces:**
- Consumes: `git2::Repository::discover(path)` — walks up from `path` to find a `.git` dir; returns `Err` if none found
- Consumes: `git2::Repository::workdir() -> Option<&Path>` — returns `None` for bare repos
- Consumes: `extract_metadata(path: &Path, root: &Path) -> anyhow::Result<ProjectMetadata>` (from `lsproj::metadata`)
- Consumes: `apply_filters(projects: Vec<ProjectMetadata>, filters: &[String]) -> Vec<ProjectMetadata>` (defined in `main.rs:137`)
- Consumes: `output::print_json`, `output::print_csv`, `output::print_table` (already used in `main.rs:128-132`)

- [ ] **Step 3: Insert the discovery check in `src/main.rs`**

Find this block in `main()` (currently lines 79–82):

```rust
    let scan_dir = args.dir.unwrap_or_else(|| PathBuf::from("."));
    let root_dir = tokio::fs::canonicalize(&scan_dir).await?;

    let tasks: Arc<Mutex<Vec<JoinHandle<()>>>> = Arc::new(Mutex::new(Vec::new()));
```

Replace with:

```rust
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
```

No new `use` statements needed — `git2` is accessed by full path and all other
identifiers are already in scope.

- [ ] **Step 4: Build to check for compile errors**

```bash
cargo build 2>&1
```

Expected: compiles cleanly with no errors or warnings.

- [ ] **Step 5: Run the new tests to confirm they pass**

```bash
cargo test test_discover_
```

Expected: both `test_discover_from_repo_root` and `test_discover_from_inside_repo` PASS.

- [ ] **Step 6: Run the full test suite to confirm no regressions**

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 7: Smoke test manually**

```bash
# From inside this repo (which is itself a git repo):
cargo run -- . --json | head -5
```

Expected: JSON array with one element whose `name` is `look_up_for_dot_git` (or whatever the current repo directory is named).

- [ ] **Step 8: Commit**

```bash
git add src/main.rs tests/cli.rs
git commit -m "feat: detect enclosing git repo when scanning from inside a repo"
```
