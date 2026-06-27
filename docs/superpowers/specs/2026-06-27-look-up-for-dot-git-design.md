# Design: Git Repo Discovery from Inside or At a Repo Root

*Date: 2026-06-27*

## Problem

`lsproj` traverses a scan root and classifies each subdirectory as a project or a
collection. It never classifies the scan root itself. This means that when the scan
root is a git repo (or is inside one), `lsproj` misreports: subdirectories of the
repo that contain files get classified as projects, and the actual repo root is never
reported at all.

**Affected cases:**

- `lsproj ~/projects/myrepo` — scan root IS the repo root
- `lsproj ~/projects/myrepo/src` — scan root is INSIDE the repo
- `lsproj .` from anywhere inside a git repo

**Expected behavior in all three cases:** report the enclosing git repo as a single
project and exit. No walk needed.

**Normal case unaffected:** when the scan root is outside any git repo (e.g.,
`lsproj ~/projects`), existing traversal behavior is unchanged.

## Non-goals

- Submodules, worktrees, subtrees: no change needed. Submodule directories already
  have `.git` as a file, which `classify_entry` already skips. Subtrees have no
  special marker. Worktrees are also already skipped.
- Bare repos: `git2::Repository::discover` returns a repo with no `workdir()`; these
  fall through to the normal walk (they have no working tree to report).
- Multi-path CLI (`lsproj path1 path2 ...`): not in scope. See Future Work below.

## Design

### Where the change lives

Entirely in `main()`. No changes to `filter.rs`, `walk_dir`, or `classify_entry`.

### Discovery check

After the `--schema` early-return and the `mark` subcommand dispatch (both of which
already short-circuit before any scanning), and after computing `root_dir`, attempt
git repo discovery before constructing walk tasks:

```rust
if let Ok(repo) = git2::Repository::discover(&root_dir) {
    if let Some(workdir) = repo.workdir() {
        let workdir = workdir.to_path_buf();
        let parent = workdir.parent().unwrap_or(&workdir).to_path_buf();
        let meta = task::spawn_blocking(move || {
            extract_metadata(&workdir, &parent)
        })
        .await??;
        let results = apply_filters(vec![meta], &args.filter);
        match (args.json, args.csv) {
            (true, _) => output::print_json(&results),
            (_, true) => output::print_csv(&results),
            _ => output::print_table(&results),
        }
        return Ok(());
    }
}
// fall through: normal async walk
```

`git2::Repository::discover` handles all git edge cases: `GIT_DIR` env var, `.git`
files, config-based `core.worktree`, etc.

`extract_metadata` is called with `(workdir, workdir.parent())` so that
`simplified_repo_path` produces just the repo's directory name — the same display
format a normal scan produces (e.g., `myrepo`, not an absolute path).

`extract_metadata` is wrapped in `spawn_blocking` for consistency with all other
`git2` usage in the codebase.

### Output

A single-element `Vec<ProjectMetadata>` passes through `apply_filters` and the same
`(args.json, args.csv)` dispatch as the normal walk. `--json`, `--csv`, `--filter`,
and `--schema` all behave identically to a normal scan.

### Error handling

`discover` failure means "not in a git repo" — fall through to normal walk, no error.

If `extract_metadata` fails on the discovered repo, the error propagates via `?` and
`main` returns an error, same as any `extract_metadata` failure in the normal walk.

## Testing

Two new integration test cases in `tests/cli.rs`:

1. **Scan from repo root:** create a temp git repo with a file, run
   `lsproj <repo_root>`, assert output contains the repo name and exit code 0.
2. **Scan from inside repo:** create a temp git repo, create a subdirectory with a
   file, run `lsproj <repo_root>/subdir`, assert output contains the repo name (not
   `subdir`).

Both cases verify the output against the table format (default). The existing
integration tests are unaffected since the normal walk path is not touched.

## Future Work

- **Multi-path CLI:** add support for `lsproj path1 path2 ...` (e.g., `lsproj *`).
  When multiple paths are provided, run the discovery check for each. A cache of
  already-discovered repo roots (checked by canonical path prefix) could avoid
  redundant `discover()` calls when many paths fall under the same repo, though
  `discover()` is fast enough that this is only worth adding if profiling shows it
  matters.
