// Sample code for a filter

use async_std::path::Path;
use git2::Repository;
use std::collections::HashSet;

pub trait Filter<T: ?Sized> {
    // ?Sized is needed to relax the usual Sized requirement on Trait params
    // since we'll be filtering Paths (which are not Sized)
    fn filter(&self, t: &T) -> bool;
}

// AddToGithub - Filter that retains paths to directories
// containing git repos that haven't yet been uploaded
pub struct AddToGithub {
    bad_path_components: HashSet<String>,
}

impl AddToGithub {
    pub fn new<S: AsRef<str>>(list: &[S]) -> Self {
        let bad_path_components = list.iter().map(|s| s.as_ref().to_owned()).collect();
        AddToGithub {
            bad_path_components,
        }
    }
}

// Filter should return true if:
// 1. last path component isn't "bad"
//      (e.g. "target", "Build", ".builds", ...)
//      This avoids repos that might be dependency checkouts.
// 2. dir contains a git repo
// 3. repo is not empty
// 4. dir does not have an 'origin' remote
//
// TODO: use regexp matching instead of bad_path_components in step 1
// TODO: origin is github.com/<username> and repo contains newer commits than origin
impl Filter<Path> for AddToGithub {
    fn filter(&self, path: &Path) -> bool {
        // 1. dir doesn't contain any "bad" path components
        if path
            .file_name()
            .and_then(|f| f.to_str())
            .is_some_and(|fname| self.bad_path_components.contains(fname))
        {
            return false;
        }

        // 2. dir contains a git repo
        let repo = match Repository::open(path) {
            Ok(r) => r,
            Err(_) => return false,
        };

        // 3. repo is not empty
        if repo.is_empty().unwrap_or(true) {
            return false;
        }

        // 4. dir does not have an 'origin' remote
        if repo.find_remote("origin").is_ok() {
            return false;
        }

        true
    }
}

/// returns a simplified absolute repo path by:
/// 1. removing the common base (i.e. the starting dir from the cmd line)
/// 2. removing the .git component (not needed, since all output paths are repos)
///
/// # Example
/// ```
/// use gitfinder::simplified_repo_path;
/// use async_std::path::Path;
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
    use git2::{Repository, Signature, Time};
    use tempfile::TempDir;

    fn make_repo_with_commit(dir: &std::path::Path) -> Repository {
        let repo = Repository::init(dir).unwrap();
        {
            let sig = Signature::new("Test", "t@t.com", &Time::new(0, 0)).unwrap();
            let tree_oid = repo.treebuilder(None).unwrap().write().unwrap();
            let tree = repo.find_tree(tree_oid).unwrap();
            repo.commit(Some("refs/heads/main"), &sig, &sig, "init", &tree, &[])
                .unwrap();
        }
        repo
    }

    #[test]
    fn test_ignore_zero_paths() {
        let tmp = TempDir::new().unwrap();
        make_repo_with_commit(tmp.path());
        let filter_dir = AddToGithub::new::<&str>(&[]);
        assert!(filter_dir.filter(Path::new(tmp.path())));
    }

    #[test]
    fn test_ignore_one_path() {
        let filter = AddToGithub::new::<&str>(&[".build"]);

        let dir = Path::new("/Users/pete/projects/net/.build");
        assert!(!filter.filter(dir));
    }

    #[test]
    fn test_ignore_multiple_paths() {
        let filter = AddToGithub::new::<&str>(&[".build", "target"]);

        let dir = Path::new("/Users/pete/projects/net/.build");
        assert!(!filter.filter(dir));
        let dir = Path::new("/Users/pete/projects/net/target");
        assert!(!filter.filter(dir));
    }

    #[test]
    fn test_is_not_dir() {
        let filter_dir = AddToGithub::new::<&str>(&[]);
        let bogus_dir = Path::new("/Users/no_such_user/");
        assert!(!filter_dir.filter(bogus_dir));
    }

    #[test]
    fn test_is_not_a_repo() {
        let filter_dir = AddToGithub::new::<&str>(&[]);
        let dir = Path::new("/Users/pete/");
        assert!(!filter_dir.filter(dir));
    }

    #[test]
    fn test_is_a_repo_with_origin() {
        let tmp = TempDir::new().unwrap();
        let repo = make_repo_with_commit(tmp.path());
        repo.remote("origin", "https://example.com/r.git").unwrap();
        let filter_dir = AddToGithub::new::<&str>(&[]);
        assert!(!filter_dir.filter(Path::new(tmp.path())));
    }

    #[test]
    fn test_is_a_repo_without_origin() {
        let tmp = TempDir::new().unwrap();
        make_repo_with_commit(tmp.path());
        let filter_dir = AddToGithub::new::<&str>(&[]);
        assert!(filter_dir.filter(Path::new(tmp.path())));
    }

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
