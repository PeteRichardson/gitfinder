# lsproj MCP Server — Design

## Overview

The lsproj MCP server wraps the `lsproj` CLI binary and exposes its output as callable tools
for use by Claude Code skills and other MCP clients. The server is intentionally thin: it
shells out to `lsproj`, parses the JSON output, and returns it. No logic is duplicated
between the binary and the server.

---

## Implementation Approach

A minimal Rust MCP server using `rmcp` (consistent with your existing MCP work). The server
has two tools:

### Tool 1: `lsproj_scan`

Scans one or more directories and returns metadata for all discovered projects.

**Input schema:**
```json
{
  "type": "object",
  "properties": {
    "paths": {
      "type": "array",
      "items": { "type": "string" },
      "description": "Directory paths to scan. Defaults to ['.'] if empty.",
      "default": ["."]
    },
    "filter": {
      "type": "string",
      "enum": ["unreviewed", "pending", "skip", "ready", "posted", "no-git"],
      "description": "Optional: restrict results to projects in this repostatus state."
    }
  },
  "required": []
}
```

**Output:** Array of `ProjectMetadata` objects (see DESIGN.md for field list).

**Implementation:**
```rust
// shells out to:
lsproj --json [--filter <filter>] <path1> <path2> ...
```

### Tool 2: `lsproj_inspect`

Runs metadata extraction on a single known project folder. Useful when the skill already
knows which project to triage and doesn't need a full scan.

**Input schema:**
```json
{
  "type": "object",
  "properties": {
    "path": {
      "type": "string",
      "description": "Absolute or relative path to the project folder."
    }
  },
  "required": ["path"]
}
```

**Output:** Single `ProjectMetadata` object.

**Implementation:**
```rust
// shells out to:
lsproj --json <path>
// returns first (and only) element of result array
```

---

## `--schema` Flag

`lsproj --schema` prints the JSON Schema for a single `ProjectMetadata` object to stdout.
The MCP server reads this at startup to build its output schema declarations dynamically,
so the schema stays in sync with the binary automatically.

```
$ lsproj --schema
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "ProjectMetadata",
  "type": "object",
  "properties": {
    "path": { "type": "string" },
    "name": { "type": "string" },
    "is_git": { "type": "boolean" },
    ...
  }
}
```

---

## Server Structure

```
lsproj-mcp/           (separate crate in the workspace, or subdir of gitfinder repo)
├── Cargo.toml
└── src/
    └── main.rs       (rmcp server, two tool handlers, shells out to lsproj binary)
```

Or as a workspace:

```toml
# Cargo.toml (workspace root)
[workspace]
members = ["lsproj", "lsproj-mcp"]
```

This keeps the binary and MCP server versioned together without conflating their code.

---

## Registration in Claude Code

In `.claude/settings.json` (or `~/.claude/settings.json` for global availability):

```json
{
  "mcpServers": {
    "lsproj": {
      "command": "/path/to/lsproj-mcp",
      "args": [],
      "env": {}
    }
  }
}
```

After registration, Claude Code skills can call:
- `mcp__lsproj__lsproj_scan`
- `mcp__lsproj__lsproj_inspect`

---

## Error Handling

| Condition | Behavior |
|---|---|
| `lsproj` binary not found | Return MCP error with install instructions |
| Path does not exist | Return MCP error with path echoed |
| Path is a file, not directory | Return MCP error |
| `lsproj` exits non-zero | Propagate stderr as MCP error message |
| JSON parse failure | Return MCP error with raw output for debugging |

---

## Future Tools (not in v1)

- `lsproj_summarize` — aggregate stats across a scan (language breakdown, total LOC, count by state)
- `lsproj_diff` — compare two scans to surface new/removed/changed projects
