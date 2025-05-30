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
// 3. dir does not have an 'origin' remote
//
// TODO: use regexp matching instead of bad_path_components in step 1
// TODO: origin is github.com/<username> and repo contains newer commits than origin
impl Filter<Path> for AddToGithub {
    fn filter(&self, path: &Path) -> bool {
        // 1. dir doesn't contain any "bad" path components
        if let Some(fname) = path.file_name().and_then(|f| f.to_str()) {
            if self.bad_path_components.contains(fname) {
                return false;
            };
        }

        // 2. dir contains a git repo
        let repo = match Repository::open(path) {
            Ok(r) => r,
            Err(_) => return false,
        };

        // 3. dir does not have an 'origin' remote
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

// NOTE: the tests below depend on some specific folders and git repos
//       on my system.  e.g. /Users/pete/practice/rust/size is a repo
//       but has never been uploaded to github, so for the purposes
//       of this filter, the expected output is true.  i.e. it is
//       a dir, doesn't contain bad components, contains a git repo
//       and doesn't have an 'origin' remote
// TODO: Change these tests to dynamically generate the necessary test
//       repos at test setup time if they don't exist already.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ignore_zero_paths() {
        let filter_dir = AddToGithub::new::<&str>(&[]);
        let dir = Path::new("/Users/pete/practice/rust/size");
        assert!(filter_dir.filter(&dir));
    }

    #[test]
    fn test_ignore_one_path() {
        let filter = AddToGithub::new::<&str>(&[".build"]);

        let dir = Path::new("/Users/pete/projects/net/.build");
        assert_eq!(filter.filter(&dir), false);
    }

    #[test]
    fn test_ignore_multiple_paths() {
        let filter = AddToGithub::new::<&str>(&[".build", "target"]);

        let dir = Path::new("/Users/pete/projects/net/.build");
        assert_eq!(filter.filter(&dir), false);
        let dir = Path::new("/Users/pete/projects/net/target");
        assert_eq!(filter.filter(&dir), false);
    }

    #[test]
    fn test_is_not_dir() {
        let filter_dir = AddToGithub::new::<&str>(&[]);
        let bogus_dir = Path::new("/Users/no_such_user/");
        assert_eq!(filter_dir.filter(&bogus_dir), false);
    }

    #[test]
    fn test_is_not_a_repo() {
        let filter_dir = AddToGithub::new::<&str>(&[]);
        let dir = Path::new("/Users/pete/");
        assert_eq!(filter_dir.filter(&dir), false);
    }

    #[test]
    fn test_is_a_repo_with_origin() {
        let filter_dir = AddToGithub::new::<&str>(&[]);
        let dir = Path::new("/Users/pete/practice/practice-rs/");
        assert_eq!(filter_dir.filter(&dir), false);
    }

    #[test]
    fn test_is_a_repo_without_origin() {
        let filter_dir = AddToGithub::new::<&str>(&[]);
        let dir = Path::new("/Users/pete/practice/rust/size/");
        assert!(filter_dir.filter(&dir));
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
