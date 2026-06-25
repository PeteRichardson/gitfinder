# lsproj — Design Proposal

## Overview

`lsproj` is a Rust CLI tool that traverses a set of local directories, identifies project roots,
and extracts structured metadata about each project. It replaces `gitfinder` with a broader
scope: any folder with files is a potential project, not just git repos. Metadata is emitted
as a formatted table (default), JSON, or CSV for consumption by automation and AI-powered
triage workflows.

The name follows the Unix `ls*` convention (`lsof`, `lsblk`, `lspci`): it lists things, and
the thing it lists is projects.

---

## Motivation

`gitfinder` solved a specific problem: find local git repos with no remote. `lsproj` generalizes
this into a metadata extraction layer that feeds a broader workflow:

1. **lsproj** — fast, deterministic, filesystem-level facts about each project folder; also
   writes `.repostatus` when the user wants to mark a project quickly from the terminal
2. **`.repostatus`** — per-project annotation file, written by `lsproj mark` or by the triage skill
3. **triage skill** — Claude Code skill that reads lsproj output + repo contents and writes `.repostatus`
   for cases requiring AI judgment

This separation keeps the binary responsible for facts + quick human triage, and the skill
responsible for cases where AI judgment adds value.

---

## Behavioral Specification

### Directory Traversal

`lsproj` accepts zero or more directory paths as positional arguments. If none are given, `.` is assumed.

For each path in the argument list:

- If the directory contains **only subdirectories and no files** (excluding hidden files and
  known noise like `.DS_Store`), treat it as a **collection root**: push all immediate
  subdirectories onto the work queue and continue.
- If the directory contains **any files**, treat it as a **project root** and run metadata
  extraction on it.

This handles the common case where `~/projects`, `~/practice`, `~/littletools` are collections
of projects rather than projects themselves, without requiring the user to enumerate every
leaf folder.

**Ignored directory names** (never descended into, never reported):
`target`, `.build`, `node_modules`, `vendor`, `.git`, `.cache`

**Worktree detection:** A directory containing a `.git` *file* (rather than a `.git` directory)
is a linked worktree checkout of another repository. These should be skipped during traversal —
the parent repo will be found and reported separately. Detect via `fs::metadata(".git").is_file()`.

**Cycle detection:** canonical path tracking to avoid symlink loops.

### Metadata Extraction

For each project root, extract:

| Field | Source | Notes |
|---|---|---|
| `path` | filesystem | Path relative to the scan root(s) |
| `name` | filesystem | Basename of the project folder |
| `is_git` | `.git/` presence or `.git` file | Boolean; `.git` file (not dir) means worktree checkout |
| `is_worktree` | `.git` file vs directory | True if this is a linked worktree; skip in traversal |
| `has_remote` | git2 | Boolean; false if not a git repo |
| `origin_url` | git2 remote named `origin` | e.g. `git@github.com:PeteRichardson/foo.git`; null if no `origin` remote |
| `is_on_github` | origin_url parse | True if origin_url contains `github.com` |
| `unpushed_count` | git2, all branches | Commits reachable from any local branch not reachable from any remote ref |
| `oldest_unpushed` | git2, all branches | Earliest date among all unpushed commits across all branches |
| `newest_unpushed` | git2, all branches | Latest date among all unpushed commits across all branches |
| `branches_with_unpushed` | git2 | Branch names that have ≥1 unpushed commit |
| `total_commits` | git2, all branches | Total commits reachable from any local branch ref |
| `loc` | tokei | Per-language line counts (code, comments, blanks) |
| `languages` | tokei | All detected languages with per-language LOC breakdown |
| `has_readme` | filesystem | Boolean |
| `has_tests` | filesystem | Presence of `tests/`, `test/`, `*_test.*`, `*_spec.*` |
| `has_ci` | filesystem | Presence of `.github/workflows/`, `.travis.yml`, etc. |
| `has_license` | filesystem | Presence of `LICENSE*` |
| `last_modified` | filesystem | mtime of most recently modified tracked file |
| `repostatus_state` | `.repostatus` | Current triage state, or `unreviewed` if absent |
| `repostatus_age_days` | `.repostatus` | Days since last reviewed, or null |

### `.repostatus` — Reading and Writing

If a `.repostatus` file exists in the project root, `lsproj` reads its `state:` and `reviewed:`
fields and includes them in scan output. This lets `lsproj` be used to filter and prioritize:

```
lsproj ~/projects --filter unreviewed
lsproj ~/projects --filter ready
```

Valid states: `unreviewed` (implicit, no file), `pending`, `skip`, `ready`, `posted`

`lsproj` also writes `.repostatus` via the `mark` subcommand, for fast terminal-based triage:

```
lsproj mark . skip "trivial hello world"
lsproj mark . skip "feature spike, not original"
lsproj mark . pending "real scope but needs docs"
lsproj mark . ready
```

**`mark` subcommand spec:**

```
lsproj mark <path> <state> [reason]
```

- `<path>`: project directory (`.` is common)
- `<state>`: one of `pending`, `skip`, `ready`, `posted`
- `[reason]`: optional short string written to `reason:` field
- Always writes `reviewed: <today>` in ISO 8601 format
- If `.repostatus` already exists, updates `state:`, `reason:`, and `reviewed:` in place;
  preserves `notes:` and any other fields the skill may have written
- If file does not exist, creates it with just the provided fields

This covers the fast human path: walk through a folder of projects, `ls` briefly, and
`lsproj mark . skip "trivial"` in a few seconds without opening a Claude session.

### Output Formats

**Default (formatted table):**
```
PATH                        LANG     LOC   COMMITS  UNPUSHED  STATUS
foo/bar                     Rust    1240       142        12   unreviewed
foo/helloworld              C         18         3         3   skip
littletools/csvdiff         Python   340        28        28   ready
```

**`--json`:** Array of objects with all fields. Suitable for piping to the triage skill.

**`--csv`:** Header row + data rows. Compatible with existing gitfinder consumers.

**`--schema`:** Prints a JSON Schema describing the output object shape. Used by the MCP
server wrapper to describe tool outputs without hardcoding field names.

### Filtering

`--filter <state>` restricts output to projects in a given `.repostatus` state.
Multiple values: `--filter unreviewed --filter pending`

`--filter no-git` — projects with no `.git` directory (candidates for `git init`)

### Exit Codes

| Code | Meaning |
|---|---|
| 0 | Success, ≥1 projects found |
| 1 | No projects found |
| 2 | Argument or I/O error |

---

## Implementation Notes

### Language / Crates

Continuing the existing Rust codebase. Suggested crate additions or replacements:

| Purpose | Crate |
|---|---|
| CLI parsing | `clap` (already used) |
| Git operations | `git2` (already used) |
| Async traversal | `tokio` + `walkdir`, or replace `async-std` with `tokio` for ecosystem consistency |
| JSON output | `serde` + `serde_json` |
| Table output | `comfy-table` or `tabled` |
| YAML parsing (`.repostatus`) | `serde_yaml` |
| LOC counting + language detection | `tokei` (via `tokei` crate as a library) |

`tokei` replaces the file-extension heuristic entirely. As a library it provides accurate
per-language LOC (code lines, comment lines, blank lines), handles polyglot projects
correctly, and skips vendored/generated files by default. The `primary_language` field
becomes the language with the most code lines according to tokei.

### Renaming

The binary target in `Cargo.toml` should be renamed from `gitfinder` to `lsproj`:

```toml
[[bin]]
name = "lsproj"
path = "src/main.rs"
```

The crate name can remain `gitfinder` or be renamed to `lsproj` — rename it to avoid confusion.

### Struct Shape (sketch)

```rust
#[derive(Serialize)]
pub struct LanguageStat {
    pub name: String,
    pub code: u64,
    pub comments: u64,
    pub blanks: u64,
}

#[derive(Serialize)]
pub struct ProjectMetadata {
    pub path: String,
    pub name: String,
    pub is_git: bool,
    pub is_worktree: bool,             // .git file (not dir) → linked worktree, skip in scan
    pub has_remote: bool,
    pub origin_url: Option<String>,    // URL of remote named "origin"
    pub is_on_github: bool,
    pub unpushed_count: u32,           // across all local branches
    pub oldest_unpushed: Option<String>,   // ISO 8601, across all branches
    pub newest_unpushed: Option<String>,   // ISO 8601, across all branches
    pub branches_with_unpushed: Vec<String>,
    pub total_commits: u32,            // reachable from any local branch
    pub primary_language: Option<String>,  // highest code-line count per tokei
    pub languages: Vec<LanguageStat>,
    pub has_readme: bool,
    pub has_tests: bool,
    pub has_ci: bool,
    pub has_license: bool,
    pub last_modified: String,         // ISO 8601
    pub repostatus_state: String,      // "unreviewed" if no file
    pub repostatus_age_days: Option<u32>,
}
```

---

## `.repostatus` File Format

```yaml
state: skip                          # unreviewed | pending | skip | ready | posted
reason: trivial spike
effort: low                          # low | medium | high (estimated work to post)
reviewed: 2026-06-23
notes: |
  No docs or tests. Dependencies likely stale. Only worth posting if updated.
```

The `lsproj_snapshot` section has been intentionally omitted. Snapshot values
(LOC, language, commit count) change over time and become misleading quickly —
a README added 10 minutes after a snapshot makes the recorded `has_readme: false`
wrong immediately. Since `lsproj` is fast, always re-run it to get current values.

`.repostatus` is in `.gitignore` because its contents are meaningful only on the machine
where the local work exists. This is intentional and correct: the triage states map cleanly
to machine-locality. `skip` repos are never posted so can never be cloned. `posted` repos,
if cloned to a new machine, are trivially re-markable (`lsproj mark . posted`). `ready`,
`pending`, and `unreviewed` reflect local unpushed work that doesn't exist on a fresh clone —
losing those states on clone is correct behavior, not data loss.

Add to each repo's `.gitignore` (or `~/.gitignore_global`):
```
.repostatus
```

---

## Migration from gitfinder

- Rename the binary target in `Cargo.toml`: `name = "lsproj"`
- Rename the crate from `gitfinder` to `lsproj` to avoid confusion
- The `AddToGithub` filter type in `src/lib.rs` → rename to `ProjectFilter` or similar
- The `simplified_repo_path` logic carries over unchanged
- The async semaphore concurrency model carries over unchanged
- The single-branch `main`/`master` commit walk is replaced by the all-branches unpushed logic

---

## Resolved Design Decisions

**1. Does `lsproj` write `.repostatus`?**
Yes, via the `mark` subcommand. The fast human triage path (`lsproj mark . skip "trivial"`)
needs to work without opening a Claude session. The skill handles the AI judgment path.
Both write the same `.repostatus` format; the skill may write richer `notes:` than the
terminal path typically will.

**2. Snapshot of metadata in `.repostatus`?**
No. LOC, language, commit count, and presence of README/tests all change over time.
A snapshot becomes stale immediately and misleads future review. `lsproj` is fast — just
re-run it. Use git history to understand what changed. The `.repostatus` file stores only
human/AI judgment (state, reason, effort, notes), not facts that the tool can re-derive.

**4. Branch coverage for unpushed commits?**
Walk all local branches, not just `main`/`master`. The correct metric is: *any commit reachable
from any local branch ref that is not reachable from any remote ref*. This catches the case
where significant work lives on a feature branch and main is clean — which would otherwise make
the project appear empty. `unpushed_count`, `oldest_unpushed`, and `newest_unpushed` all
aggregate across branches. `branches_with_unpushed` lists which branches have unpushed work,
giving the triage skill something concrete to report ("3 branches, 100 unpushed commits").

**5. Worktree handling?**
Linked worktrees (created by `git worktree add`) have a `.git` *file* rather than a `.git`
directory. Skip any directory where `.git` is a file during traversal.

Worktrees are not separate repos — they are additional working trees of the *same* repo,
sharing the same `.git` object store and all the same refs. When `lsproj` scans the repo
root, it already sees every branch including those checked out in worktrees. The worktree
directory itself adds no new commits or refs. So skipping worktree directories during
traversal produces exactly correct results: all unpushed commits on all branches are
captured via the root, with no double-counting and no misses. The `branches_with_unpushed`
field will name the branch (e.g. `fix_bug_12345`) regardless of whether it happens to be
checked out in a worktree or not — that detail is irrelevant to whether the commits are pushed.

**3. LOC counting and language detection?**
Use the `tokei` crate as a library. It provides accurate per-language stats (code, comment,
blank lines), handles polyglot projects, skips vendored/generated files, and is well-maintained.
`primary_language` is the language with the most code lines per tokei. No custom heuristics needed.

