# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
cargo build                                          # build
cargo run -- ~/projects                             # run against a directory
cargo test                                          # all tests (unit + integration + doctest)
cargo test test_finds_repo                          # run a single test by name prefix
cargo fmt --check                                   # check formatting
cargo clippy --all-targets --all-features -- -D warnings  # lint
```

## Architecture

`lsproj` is a Rust CLI that traverses local project directories and extracts metadata to support a publish-triage workflow. Currently it scans for non-empty Git repositories with no `origin` remote and prints CSV output. It is being extended toward richer metadata extraction, multiple output formats, a `mark` subcommand for writing `.repostatus` files, and a companion MCP server (`lsproj-mcp`). See `docs/lsproj_design.md` for the full spec and `docs/lsproj-PLAN.md` for the implementation plan.

**`src/lib.rs`** — library crate containing:
- `Filter<T>` trait: single method `filter(&self, t: &T) -> bool`
- `AddToGithub` struct: implements `Filter<Path>`, returning `true` when a directory is a non-empty git repo without an `origin` remote. The name reflects intent (these are repos to add to GitHub), not a generic concept.
- `simplified_repo_path`: strips the scan root prefix and `.git` suffix from a path to produce the relative display path used in CSV output.

**`src/main.rs`** — binary entry point:
- Parses a single optional `DIR` argument via `clap`.
- Async directory traversal using `async-std` (not tokio). Each subdirectory is processed in a spawned task.
- Concurrency is bounded by an `async_lock::Semaphore` (limit 100) to avoid "too many open files".
- `walk_dir` is a recursive async function, boxed with `Pin<Box<dyn Future>>` to allow recursion.
- When `AddToGithub::filter` returns `true` for a directory, `print_git_repo_info` is called — it opens the repo via `git2`, walks commits on `main` (fallback: `master`), computes oldest/newest dates and count, and prints a CSV row.
- `git2` is synchronous; all git operations run inside `spawn_blocking`.

**`tests/cli.rs`** — integration tests that build the binary and invoke it via `Command::new(env!("CARGO_BIN_EXE_lsproj"))`. Tests use `tempfile::TempDir` and `git2` directly to set up fixture repos.

## Key design notes

- The `Filter` trait returns `true` to **match** (report) a repo, not to **continue recursion**. In `walk_dir`, a `true` result triggers `print_git_repo_info`; a `false` result triggers recursive descent into the directory.
- CSV date format is `%y-%m-%d` (2-digit year), e.g. `23-11-14`.
- Commit walk is on the local `main` branch only (fallback `master`); other branches are ignored. The design calls for replacing this with an all-branches unpushed-commit walk.
- `AddToGithub` is planned to be renamed (to `ProjectFilter` or similar) as the tool broadens beyond git-only repos.
- The repo will become a Cargo workspace with `lsproj` and `lsproj-mcp` as separate crates. New dependencies coming: `tokei` (LOC/language), `serde`/`serde_json` (JSON output), `serde_yaml` (`.repostatus`), `comfy-table` or `tabled` (table output).
