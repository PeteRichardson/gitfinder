# lsproj — Implementation Plan

*Last updated: 2026-06-24*

This is a sequenced checklist for creating the lsproj repo from the existing
gitfinder codebase and building out the full design. Work top to bottom.

---

## Phase 1: Repo Setup

- [ ] Create `~/projects/lsproj/` directory (or rename/re-init from gitfinder)
- [ ] `git init` (or keep existing gitfinder history — your call)
- [ ] Create `.gitignore`:
  ```
  target/
  .DS_Store
  .repostatus
  ```
- [ ] Create directory structure:
  ```bash
  mkdir -p docs skills/triage lsproj/src lsproj-mcp/src
  ```
- [ ] Copy design docs into place:
  - `lsproj_design.md` → `docs/DESIGN.md`
  - `lsproj_mcp.md` → `docs/MCP.md`
- [ ] Copy `SKILL-triage.md` → `skills/triage/SKILL.md`
- [ ] Create repo on GitHub:
  ```bash
  gh repo create lsproj --public --source=. --push
  ```

---

## Phase 2: Rust Workspace Setup

- [ ] Create workspace `Cargo.toml` at repo root:
  ```toml
  [workspace]
  members = ["lsproj", "lsproj-mcp"]
  resolver = "2"
  ```
- [ ] Move existing gitfinder source into `lsproj/` crate:
  - Copy `src/` → `lsproj/src/`
  - Copy `Cargo.toml` → `lsproj/Cargo.toml`
- [ ] Update `lsproj/Cargo.toml`:
  - `name = "lsproj"` (package name)
  - Binary target: `[[bin]] name = "lsproj" path = "src/main.rs"`
  - Add new dependencies: `tokei`, `serde_yaml`, `comfy-table` or `tabled`
- [ ] Create stub `lsproj-mcp/Cargo.toml` and `lsproj-mcp/src/main.rs`
- [ ] Verify `cargo build` compiles cleanly with new workspace structure

---

## Phase 3: lsproj CLI — Core Changes

These implement the delta from gitfinder to lsproj per `docs/DESIGN.md`.

### Metadata expansion
- [ ] Add `LanguageStat` struct and `languages: Vec<LanguageStat>` to `ProjectMetadata`
- [ ] Integrate `tokei` as a library for LOC + language detection
- [ ] Add `primary_language` field (language with highest code-line count)
- [ ] Add `has_readme`, `has_tests`, `has_ci`, `has_license` boolean fields
- [ ] Add `last_modified` field (mtime of most recently modified tracked file)

### Unpushed commit logic
- [ ] Replace single-branch `main`/`master` walk with all-branches unpushed logic:
  - Walk all local branch refs
  - For each: find commits not reachable from any remote ref
  - Aggregate into `unpushed_count`, `oldest_unpushed`, `newest_unpushed`
  - Collect `branches_with_unpushed: Vec<String>`

### `.repostatus` support
- [ ] Add `repostatus_state` and `repostatus_age_days` fields to `ProjectMetadata`
- [ ] Read `.repostatus` YAML during metadata extraction if present
- [ ] Implement `mark` subcommand:
  ```
  lsproj mark <path> <state> [reason]
  ```
  - Writes/updates `.repostatus` with `state:`, `reason:`, `reviewed:` fields
  - Preserves existing `notes:` and other fields if file already exists

### Output formats
- [ ] Add `--json` flag: emit array of `ProjectMetadata` as JSON
- [ ] Add `--csv` flag: emit header + data rows
- [ ] Add `--schema` flag: emit JSON Schema for a single `ProjectMetadata` object
- [ ] Default table output: `PATH | LANG | LOC | COMMITS | UNPUSHED | STATUS`

### Filtering
- [ ] Add `--filter <state>` flag (repeatable)
- [ ] Add `--filter no-git` for projects without `.git`

### Traversal improvements
- [ ] Implement collection-root detection (directory with only subdirs → descend)
- [ ] Add worktree detection: skip dirs where `.git` is a file not a directory
- [ ] Add cycle detection via canonical path tracking

---

## Phase 4: lsproj-mcp Server

- [ ] Implement minimal `rmcp`-based MCP server in `lsproj-mcp/src/main.rs`
- [ ] Tool 1: `lsproj_scan`
  - Accepts `paths: Vec<String>` and optional `filter: String`
  - Shells out to `lsproj --json [--filter <f>] <paths...>`
  - Returns parsed array of `ProjectMetadata`
- [ ] Tool 2: `lsproj_inspect`
  - Accepts `path: String`
  - Shells out to `lsproj --json <path>`
  - Returns first element of result array
- [ ] Read `lsproj --schema` at startup to build output schema declarations
- [ ] Error handling per `docs/MCP.md` error table
- [ ] Verify tools appear as `mcp__lsproj__lsproj_scan` and
      `mcp__lsproj__lsproj_inspect` in Claude Code

---

## Phase 5: `/triage` Skill

- [ ] Update `skills/triage/SKILL.md` frontmatter:
  - `name: triage`
  - `disable-model-invocation: true`
  - Verify description trigger phrases are accurate
- [ ] Update body: confirm all `mcp__lsproj__lsproj_inspect` tool call names
      match actual MCP server tool names from Phase 4
- [ ] Remove bash fallback (triage is intentionally crippled without lsproj —
      fail clearly with an actionable error message instead)

---

## Phase 6: `install.sh`

- [ ] Create `install.sh` at repo root:
  ```bash
  #!/usr/bin/env bash
  set -e

  SKILLS_DIR="$HOME/.claude/skills"
  REPO_DIR="$(cd "$(dirname "$0")" && pwd)"

  # Build and install binaries
  echo "Building lsproj and lsproj-mcp..."
  cargo build --release
  cargo install --path lsproj
  cargo install --path lsproj-mcp
  echo "Installed: $(which lsproj), $(which lsproj-mcp)"

  # Symlink triage skill
  TRIAGE_SRC="$REPO_DIR/skills/triage"
  TRIAGE_DST="$SKILLS_DIR/triage"
  if [ -e "$TRIAGE_DST" ]; then
      echo "Skipping triage skill (already exists at $TRIAGE_DST)"
  else
      ln -s "$TRIAGE_SRC" "$TRIAGE_DST"
      echo "Linked: triage -> $TRIAGE_DST"
  fi

  # Remind user to register MCP server
  MCP_BIN="$(which lsproj-mcp)"
  echo ""
  echo "Next: register lsproj-mcp in ~/.claude/settings.json:"
  echo '  {'
  echo '    "mcpServers": {'
  echo '      "lsproj": {'
  echo "        \"command\": \"$MCP_BIN\","
  echo '        "args": [],'
  echo '        "env": {}'
  echo '      }'
  echo '    }'
  echo '  }'
  ```
- [ ] `chmod +x install.sh`

---

## Phase 7: Finish and Bootstrap

- [ ] Run `install.sh` end-to-end on a clean machine (or simulate)
- [ ] Verify `lsproj ~/projects` produces correct table output
- [ ] Verify `lsproj --json ~/projects` pipes correctly
- [ ] Verify `lsproj mark . skip "trivial"` writes `.repostatus`
- [ ] Verify Claude Code sees `mcp__lsproj__lsproj_scan` and `mcp__lsproj__lsproj_inspect`
- [ ] Run `/triage` on a test project and verify end-to-end flow
- [ ] Run `/readme` on the lsproj repo itself to generate `README.md`
- [ ] Initial commit and push

---

## Notes

**gitfinder migration:** The existing gitfinder codebase provides the git2 integration,
async traversal, and basic metadata extraction. The changes in Phase 3 are additive
rather than rewrites — keep what works, extend what's missing.

**`.repostatus` in `.gitignore_global`:** Add `.repostatus` globally so every repo
automatically excludes it without per-repo `.gitignore` edits:
```bash
echo ".repostatus" >> ~/.gitignore_global
git config --global core.excludesfile ~/.gitignore_global
```

**rmcp version:** Use `rmcp` v1.7.0+. API has breaking changes from earlier versions:
`Parameters<T>` wrapper and `#[tool_router(server_handler)]` attribute are required.
See existing MCP server work for reference patterns.
