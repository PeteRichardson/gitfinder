---
name: triage
description: >
  Triage a local project folder to determine whether it's worth saving (private)
  or publishing (public) on GitHub. Reads project metadata via lsproj, inspects
  repo contents, and writes or updates a .repostatus file with a recommended state,
  effort estimate, and structured notes. Triggered by /triage or "triage this project"
  or "assess this repo".
version: 0.1.0
tools_required:
  - mcp__lsproj__lsproj_inspect   # lsproj MCP server
  - read_file
  - list_directory
  - bash
---

# /triage Skill

## Purpose

Assess a single project folder and produce a `.repostatus` file that captures:
- Whether the project is worth posting (public or private)
- Why or why not
- How much effort posting would take
- What's specifically missing

This skill is designed to be run in a loop across many projects. Keep assessments
concise and consistent so they're comparable across projects.

---

## Steps

### 1. Identify the target directory

If the user invoked `/triage` from within a project folder, use the current directory.
If a path was provided (e.g. `/triage ~/projects/foo`), use that path.
If neither, ask: "Which project folder should I triage?"

### 2. Check for existing `.repostatus`

Read `.repostatus` if present. If `state` is `posted` or `skip`, confirm with the user
before re-triaging:
> "This project is already marked as `{state}` (reviewed {age} days ago). Re-triage anyway?"

If the user says no, stop here.

> **Note:** For obviously trivial projects the user doesn't need AI help with, `lsproj mark`
> is faster than invoking this skill: `lsproj mark . skip "trivial"`. This skill is most
> valuable when the assessment requires reading code and applying judgment.

### 3. Collect structured metadata via lsproj

Call `mcp__lsproj__lsproj_inspect` with the target path.

Key fields to note for assessment:
- `is_git`, `has_remote`, `is_on_github` — where does it stand in the posting pipeline?
- `unpushed_count`, `oldest_unpushed`, `newest_unpushed` — scope of unpushed work
- `total_commits` — indicator of sustained effort vs. one-off spike
- `primary_language`, `languages` — what it is, with per-language code line counts from tokei
- `loc` — per-language breakdown; use code lines (not total lines) as the size signal
- `has_readme`, `has_tests`, `has_ci`, `has_license` — posting readiness gaps
- `last_modified` — is this actively maintained or abandoned?

### 4. Inspect repo contents

Use `list_directory` and targeted `read_file` calls to gather qualitative signals:

- **README.md** (if present): What does the project claim to do? Is it coherent?
- **Top-level file structure**: Does it look like a finished tool, a spike, or a learning exercise?
- **`src/` or equivalent**: Is there a clear entry point? Multiple modules suggest real scope.
- **Cargo.toml / package.json / pyproject.toml**: Project name, version, dependencies.
  Are dependencies current or years out of date?
- **Commit messages** (via `bash -c "git log --oneline -20"`): Do they tell a story of
  purposeful development, or is it "initial commit" + silence?
- **Test files**: Are tests present and non-trivial?

Do not read every file. Use judgment — a 3-file Python script needs less inspection than
a multi-crate Rust workspace.

### 5. Assess significance

Answer these questions based on gathered evidence:

**Scope:**
- Is this a complete, working tool/library with a clear purpose?
- Or is it a throwaway spike, hello-world variant, or feature exploration?

**Effort invested:**
- Does commit count + LOC + date range suggest sustained work?
- Would someone (including future-you) find this useful or instructive?

**Posting value:**
- Public: Is this original enough or well-documented enough to be useful to others?
- Private: Does it represent meaningful personal work worth archiving?
- Even a 200-line well-commented spike on an interesting problem may be worth private posting.
  Even a 2000-line script that duplicates existing tools is worth private posting if it represents real effort.

**Effort to post:**
- **low**: Has README, license, tests, recent deps. Could post as-is or with minor cleanup.
- **medium**: Missing README or tests. Deps may need updating. A few hours of work.
- **high**: No docs, no tests, stale deps, or significant refactoring needed before it's
  presentable even privately.

### 6. Determine recommended state

| Condition | Recommended state |
|---|---|
| Already on GitHub | `posted` |
| Worth posting, ready or near-ready | `ready` |
| Worth posting, significant work needed | `pending` |
| Not worth posting (trivial, duplicated, or abandoned) | `skip` |
| Genuinely unclear — needs more thought | `pending` |

Default toward `pending` when uncertain. `skip` should require clear reasoning.

### 7. Write `.repostatus`

Write the file to the project root. Preserve any existing `notes` from a prior assessment
(append rather than replace if re-triaging).

```yaml
state: {state}
reason: {one-line summary}
effort: {low|medium|high}
reviewed: {today's date in YYYY-MM-DD}
notes: |
  {2-5 sentences. What the project does. Why this assessment.
   What specifically is missing if effort > low.
   Any interesting technical details worth remembering.}
```

### 8. Report to user

Print a compact summary:

```
✓ Triaged: foo/bar
  State:   pending
  Effort:  medium
  Reason:  Rust CLI with real scope (340 LOC, 28 commits) but no README or tests.
           Dependencies from 2021 — likely need updating before posting.
```

Then ask: "Accept this assessment, or would you like to adjust the state or notes?"

If the user adjusts, update `.repostatus` and confirm.

---

## Iteration Notes

This skill is designed to evolve. When the assessment feels wrong:

- If the skill lacked data to decide → note which field would have helped, and consider
  adding it to `lsproj`'s extraction (open an issue or add to DESIGN.md).
- If the reasoning was off → update the assessment criteria in Step 5 of this skill.
- After triaging 10+ projects, review the distribution of states and effort levels.
  If everything is `pending/medium`, the heuristics may need recalibration.

---

## Batch Triage (Advanced)

To triage multiple projects in sequence, call `mcp__lsproj__lsproj_scan` with a collection
root and filter `unreviewed`, then invoke this skill's logic for each result in priority order:

Priority order (highest first):
1. Has unpushed commits + high LOC + recent activity (`last_modified` < 90 days)
2. Has unpushed commits + moderate LOC
3. Has unpushed commits, low LOC
4. No git, has files (potential `git init` candidate)
5. Has remote, already on GitHub (verify `posted` state)

Present the user with the top 5 candidates and let them pick which to triage next,
rather than running fully autonomously. This keeps the human in the loop for judgment calls.
