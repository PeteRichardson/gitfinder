# Project Root Filter Rules

Rules for excluding folder paths that are definitely **not** software project roots.
Apply these before presenting candidate folders to the user or posting to GitHub.

## 1. Skip if any path component matches exactly

| Component | Reason |
|-----------|--------|
| `Build` | Xcode build artifact trees |
| `.venv` | Python virtual environments |
| `.vscode` | VS Code editor config folders |
| `.cpcache` | Clojure tools cache |
| `node_modules` | Node.js dependencies |
| `__pycache__` | Python bytecode cache |
| `target` | Rust build output |
| `dist` | Generic build output |
| `build` | Generic build output (case-insensitive match recommended) |
| `vendor` | Vendored dependencies |
| `.git` | Git repo metadata (the `.git` dir itself, not its parent) |

## 2. Skip if the final path component ends with

| Suffix | Reason |
|--------|--------|
| `.xcodeproj` | Xcode project metadata; parent is the real root |
| `.xcworkspace` | Same as above |
| `.noindex` | Xcode build cache directories (`ModuleCache.noindex`, `SDKStatCaches.noindex`, `SymbolCache.noindex`, `CompilationCache.noindex`) |
| `.sdk` | SDK symlink trees inside Xcode caches (may have platform suffix, e.g. `MacOSX15.4.sdk (24E241)`) — match with `contains(".sdk")` |

## 3. Depth/deduplication heuristics

- **Prefer shallower paths.** If both `connect4` and `connect4/connect4py` appear as candidates, only emit the children as individual projects — or if the parent has no code of its own, skip it and keep the children.
- **Organizational grouping folders** (`rust/`, `c/`, `clojure/`, `python/`, etc.) are not project roots themselves; their immediate children are the real candidates. Consider detecting these by checking whether the folder contains only subdirectories and no source files at the top level.

## 4. Examples from the wild

| Path | Rule that catches it |
|------|----------------------|
| `slip/.venv` | Component `.venv` |
| `testg13/Build/CompilationCache.noindex/generic` | Component `Build` |
| `xctesttest/xctesttest.xcodeproj` | Suffix `.xcodeproj` |
| `watch/button1/Build/ModuleCache.noindex` | Component `Build` AND suffix `.noindex` |
| `watch/project1/Build/SymbolCache.noindex/WatchSimulator10.5.sdk watchsimulator (21T569)` | Component `Build`, suffix `.noindex` on parent, and `.sdk` in final component |
| `clojure/s3cold/.cpcache` | Component `.cpcache` |
| `c/.vscode` | Component `.vscode` |
| `connect4/.vscode` | Component `.vscode` |