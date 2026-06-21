# gitfinder

Find local Git repositories that have commits but no `origin` remote, and print commit-date/count metadata as CSV.

## Motivation

When you have many local project folders, it is easy to lose track of repositories that were initialized and used locally but never connected to a remote. `gitfinder` scans a directory tree and reports repos that look ready to publish: non-empty repos with no `origin` remote.

## Key features

- Recursively walks a directory tree starting from a provided root directory.
- Filters for Git repositories that:
  - are not in ignored directory names (`target`, `.build`, `node_modules`, `vendor`, `.git`)
  - are not empty
  - do not have an `origin` remote
- Prints CSV output with header:
  - `repository` (path relative to scan root)
  - `oldest` commit date (`%y-%m-%d`, where `%y` is two-digit year, as implemented in `src/main.rs`)
  - `newest` commit date (`%y-%m-%d`, where `%y` is two-digit year, as implemented in `src/main.rs`)
  - `count` of commits on local `main` (fallback: `master`)
- Uses async directory traversal with bounded concurrency.

## Prerequisites

- Rust toolchain with Cargo
  - Crate edition is `2024` (`Cargo.toml`), so use a Rust version that supports edition 2024.
- Git libraries are linked through `git2`/`libgit2` from Cargo dependencies.
- Required environment variables: none.

## Installation

Build from source:

```bash
git clone https://github.com/PeteRichardson/gitfinder.git
cd gitfinder
cargo build --release
```

Run without installing:

```bash
cargo run -- /path/to/search/root
```

Optional local install via Cargo:

```bash
cargo install --path .
gitfinder /path/to/search/root
```

## Usage

CLI help:

```bash
cargo run -- --help
```

Expected help text:

```text
Walk a directory tree asynchronously with bounded concurrency

Usage: gitfinder [DIR]

Arguments:
  [DIR]  Directory to start walking from (default: ".") [default: .]

Options:
  -h, --help  Print help
```

Most common invocations:

```bash
# Scan current directory (default)
gitfinder

# Scan a specific tree
gitfinder ~/projects

# Run directly with Cargo
cargo run -- ~/projects
```

Example output format:

```text
repository,oldest,newest,count
myrepo,23-11-14,23-11-16,2
level1/level2/myrepo,23-11-14,23-11-14,1
```

## Configuration

`gitfinder` exposes one CLI argument and uses a few built-in behaviors from source code.

### Command-line interface

| Argument | Type | Default | Description |
| --- | --- | --- | --- |
| `DIR` | positional path | `.` | Root directory to scan recursively |
| `-h`, `--help` | flag | n/a | Show help text |

### Built-in behavior (from code)

| Setting | Value | Where defined |
| --- | --- | --- |
| Concurrency limit for async traversal | `100` | `src/main.rs` (`concurrency_limit`) |
| Ignored directory names during scan | `target`, `.build`, `node_modules`, `vendor`, `.git` | `src/main.rs` (`AddToGithub::new(...)`, where `AddToGithub` is the existing filter type name) |
| Branch used for commit walk | `main`, fallback `master` | `src/main.rs` (`print_git_repo_info`) |
| Date output format | `%y-%m-%d` | `src/main.rs` (`datetime.format(...)`) |

Config files: none.  
Environment variables: none.

## Architecture overview

The binary entrypoint is `src/main.rs`. It parses `DIR` with `clap`, canonicalizes the root path, prints a CSV header, and launches async recursive directory walking with `async-std`. Traversal is task-based and guarded by an `async_lock::Semaphore` (limit `100`) to avoid excessive open files.

Filtering logic lives in `src/lib.rs` via the `Filter` trait and `AddToGithub` implementation. A directory is reported only if it is a non-empty Git repository without `origin`; otherwise traversal continues into it (unless excluded by ignored directory-name components). For each matching repository, `print_git_repo_info` opens it with `git2`, walks commits from `main` or `master`, computes earliest/latest commit dates and count, and prints one CSV row using a path relative to the scan root (`simplified_repo_path`).

## Development workflow

From repository root:

```bash
# Build
cargo build

# Test (unit, integration, doctest)
cargo test

# Lint/style (standard Rust toolchain)
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

Tests include:

- unit tests in `src/lib.rs`
- CLI/integration tests in `tests/cli.rs`

## License

No license file is currently present in this repository (`LICENSE*` not found at repo root). Add a license file before distributing or reusing this code outside your own environment.
