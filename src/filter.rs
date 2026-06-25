use std::path::Path;

pub enum EntryKind {
    Skip,
    Project,
    Collection,
}

const SKIP_DIRS: &[&str] = &[
    "target",
    ".build",
    "node_modules",
    "vendor",
    ".git",
    ".cache",
];

/// Classify a directory entry for traversal.
///
/// Returns:
/// - `Skip` if the directory should be ignored entirely (known build/dep dirs, or a git worktree)
/// - `Project` if the directory is a project root (contains non-hidden files)
/// - `Collection` if the directory contains only subdirectories (descend into it)
pub fn classify_entry(path: &Path) -> EntryKind {
    // Skip known noise directories
    if let Some(name) = path.file_name().and_then(|n| n.to_str())
        && SKIP_DIRS.contains(&name)
    {
        return EntryKind::Skip;
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
