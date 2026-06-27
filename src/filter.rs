use std::path::Path;

pub enum EntryKind {
    Skip,
    Project,
    Collection,
}

// Exact final-component matches (case-sensitive). "build" is handled separately
// via case-insensitive comparison; all other entries are exact.
const SKIP_COMPONENTS: &[&str] = &[
    "target",
    ".build",
    "node_modules",
    "vendor",
    ".git",
    ".cache",
    ".venv",
    ".vscode",
    ".cpcache",
    "__pycache__",
    "dist",
];

// Final-component suffix matches (e.g. "foo.xcodeproj").
const SKIP_SUFFIXES: &[&str] = &[".xcodeproj", ".xcworkspace", ".noindex"];

/// Classify a directory entry for traversal.
///
/// Returns:
/// - `Skip` if the directory should be ignored entirely (known build/dep dirs, or a git worktree)
/// - `Project` if the directory is a project root (contains non-hidden files)
/// - `Collection` if the directory contains only subdirectories (descend into it)
pub fn classify_entry(path: &Path) -> EntryKind {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if SKIP_COMPONENTS.contains(&name)
            || name.eq_ignore_ascii_case("build")
            || SKIP_SUFFIXES.iter().any(|s| name.ends_with(s))
            || name.contains(".sdk")
        {
            return EntryKind::Skip;
        }

        // Skip Contents/ inside a .app bundle (macOS app packaging convention)
        if name == "Contents"
            && path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .map(|n| n.ends_with(".app"))
                .unwrap_or(false)
        {
            return EntryKind::Skip;
        }
    }

    // Skip git worktrees: .git is a FILE (not a dir) in a linked worktree
    let git_path = path.join(".git");
    if git_path.is_file() {
        return EntryKind::Skip;
    }

    // Git repos are always project roots, even if they have no non-hidden files yet
    if git_path.is_dir() {
        return EntryKind::Project;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn skip(name: &str) -> bool {
        // Build a fake path so classify_entry can extract file_name()
        let path = std::path::Path::new("/some/root").join(name);
        matches!(classify_entry(&path), EntryKind::Skip)
    }

    #[test]
    fn skips_exact_components() {
        for name in &[
            ".venv",
            ".vscode",
            ".cpcache",
            "__pycache__",
            "dist",
            "node_modules",
            "vendor",
            "target",
            ".git",
            ".build",
            ".cache",
        ] {
            assert!(skip(name), "expected Skip for {name}");
        }
    }

    #[test]
    fn skips_build_case_insensitive() {
        assert!(skip("build"));
        assert!(skip("Build"));
        assert!(skip("BUILD"));
    }

    #[test]
    fn skips_xcodeproj_suffixes() {
        assert!(skip("xctesttest.xcodeproj"));
        assert!(skip("MyApp.xcworkspace"));
        assert!(skip("ModuleCache.noindex"));
        assert!(skip("SDKStatCaches.noindex"));
    }

    #[test]
    fn skips_sdk_contains() {
        assert!(skip("MacOSX15.4.sdk (24E241)"));
        assert!(skip("WatchSimulator10.5.sdk watchsimulator (21T569)"));
        assert!(skip("iPhoneOS.sdk"));
    }

    #[test]
    fn skips_app_contents() {
        let path = std::path::Path::new("/Applications/MyApp.app/Contents");
        assert!(matches!(classify_entry(path), EntryKind::Skip));

        // "Contents" without a .app parent is not skipped by this rule
        let path2 = std::path::Path::new("/some/project/Contents");
        assert!(!matches!(classify_entry(path2), EntryKind::Skip));
    }

    #[test]
    fn does_not_skip_normal_dirs() {
        // "dist" is a skip dir, but unrelated names should not be skipped by the name check.
        // We can't easily test Project vs Collection here (requires real fs), but we can
        // verify that clearly non-matching names don't hit the Skip branch.
        for name in &["myproject", "connect4", "rust", "slip"] {
            assert!(!skip(name), "unexpected Skip for {name}");
        }
    }
}
