use std::path::Path;

pub mod filter;
pub mod fs_meta;
pub mod git_info;
pub mod loc;
pub mod metadata;
pub mod output;
pub mod repostatus;

/// returns a simplified absolute repo path by:
/// 1. removing the common base (i.e. the starting dir from the cmd line)
/// 2. removing the .git component (not needed, since all output paths are repos)
///
/// # Example
/// ```
/// use lsproj::simplified_repo_path;
/// use std::path::Path;
/// let simple = simplified_repo_path(
///     Path::new("/Users/pete/projects/foo/lib/.git"),
///     Path::new("/Users/pete/projects/")
/// );
/// assert_eq!(simple, "foo/lib");
/// ```
pub fn simplified_repo_path(path: &Path, base: &Path) -> String {
    // If last component is ".git", use parent; else use path directly
    let path_to_strip = match path.file_name().and_then(|f| f.to_str()) {
        Some(".git") => path.parent().unwrap(),
        _ => path,
    };
    if let Ok(display_path) = path_to_strip.strip_prefix(base) {
        return display_path.display().to_string();
    }
    // should never happen, since base is the root dir where the walk starts,
    // so all paths will have a parent and be under base.
    panic!(
        "Impossible! path ({}) has no parent or is not under base ({})!",
        path.display(),
        base.display()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_path_basic() {
        let simple = simplified_repo_path(
            Path::new("/Users/pete/projects/foo/lib/.git"),
            Path::new("/Users/pete/projects/"),
        );
        assert_eq!(simple, "foo/lib");
    }

    #[test]
    fn test_simple_path_no_dotgit() {
        let simple = simplified_repo_path(
            Path::new("/Users/pete/projects/foo/lib"),
            Path::new("/Users/pete/projects/"),
        );
        assert_eq!(simple, "foo/lib");
    }
}
