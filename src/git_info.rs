use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use git2::{BranchType, Repository};

#[derive(Default)]
pub struct GitInfo {
    pub is_git: bool,
    pub is_worktree: bool,
    pub has_remote: bool,
    pub origin_url: Option<String>,
    pub is_on_github: bool,
    pub unpushed_count: u32,
    pub oldest_unpushed: Option<String>,
    pub newest_unpushed: Option<String>,
    pub branches_with_unpushed: Vec<String>,
    pub total_commits: u32,
    pub last_modified: Option<String>,
}

pub fn extract_git_info(path: &Path) -> GitInfo {
    inner(path).unwrap_or_default()
}

fn to_iso8601(secs: i64) -> String {
    let dt = DateTime::<Utc>::from(UNIX_EPOCH + Duration::from_secs(secs.unsigned_abs()));
    dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn inner(path: &Path) -> anyhow::Result<GitInfo> {
    let repo = Repository::open(path)?;

    // origin URL
    let origin_url = repo
        .find_remote("origin")
        .ok()
        .and_then(|r| r.url().map(|s| s.to_string()));
    let has_remote = !repo.remotes()?.is_empty();
    let is_on_github = origin_url
        .as_deref()
        .map(|u| u.contains("github.com"))
        .unwrap_or(false);

    // All remote ref tips (for hiding in revwalk)
    let remote_oids: HashSet<git2::Oid> = repo
        .references()?
        .flatten()
        .filter(|r| {
            r.name()
                .map(|n| n.starts_with("refs/remotes/"))
                .unwrap_or(false)
        })
        .filter_map(|r| r.target())
        .collect();

    // Total commits: walk from all local branch tips
    let mut total_revwalk = repo.revwalk()?;
    for (branch, _) in repo.branches(Some(BranchType::Local))?.flatten() {
        if let Some(oid) = branch.get().target() {
            let _ = total_revwalk.push(oid);
        }
    }
    let total_commits = total_revwalk.count() as u32;

    // Unpushed commits: per branch, hide all remote refs
    let mut branches_with_unpushed: Vec<String> = Vec::new();
    let mut all_unpushed_secs: Vec<i64> = Vec::new();

    for branch_result in repo.branches(Some(BranchType::Local))? {
        let (branch, _) = branch_result?;
        let branch_name = branch.name()?.unwrap_or("<unnamed>").to_string();
        let tip = match branch.get().target() {
            Some(oid) => oid,
            None => continue,
        };

        let mut revwalk = repo.revwalk()?;
        revwalk.push(tip)?;
        for &remote_oid in &remote_oids {
            let _ = revwalk.hide(remote_oid);
        }

        let mut branch_secs: Vec<i64> = Vec::new();
        for oid_result in revwalk {
            let commit = repo.find_commit(oid_result?)?;
            branch_secs.push(commit.time().seconds());
        }

        if !branch_secs.is_empty() {
            branches_with_unpushed.push(branch_name);
            all_unpushed_secs.extend(branch_secs);
        }
    }

    let unpushed_count = all_unpushed_secs.len() as u32;
    let oldest_unpushed = all_unpushed_secs.iter().copied().min().map(to_iso8601);
    let newest_unpushed = all_unpushed_secs.iter().copied().max().map(to_iso8601);

    // last_modified: most recent mtime in git index
    let index = repo.index()?;
    let last_modified = index
        .iter()
        .map(|e| e.mtime.seconds() as i64)
        .max()
        .map(to_iso8601);

    Ok(GitInfo {
        is_git: true,
        is_worktree: false, // worktrees are skipped in traversal; never reported
        has_remote,
        origin_url,
        is_on_github,
        unpushed_count,
        oldest_unpushed,
        newest_unpushed,
        branches_with_unpushed,
        total_commits,
        last_modified,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::{Repository, Signature, Time};
    use tempfile::TempDir;

    fn make_repo(dir: &std::path::Path, commit_times: &[i64]) -> Repository {
        let repo = Repository::init(dir).unwrap();
        {
            let tree_oid = repo.treebuilder(None).unwrap().write().unwrap();
            let tree = repo.find_tree(tree_oid).unwrap();
            let mut parent_oid: Option<git2::Oid> = None;
            for &secs in commit_times {
                let sig = Signature::new("T", "t@t.com", &Time::new(secs, 0)).unwrap();
                let parents: Vec<git2::Oid> = parent_oid.into_iter().collect();
                let parent_commits: Vec<git2::Commit> = parents
                    .iter()
                    .map(|&o| repo.find_commit(o).unwrap())
                    .collect();
                let parent_refs: Vec<&git2::Commit> = parent_commits.iter().collect();
                let oid = repo
                    .commit(
                        Some("refs/heads/main"),
                        &sig,
                        &sig,
                        "test",
                        &tree,
                        &parent_refs,
                    )
                    .unwrap();
                parent_oid = Some(oid);
            }
        }
        repo
    }

    #[test]
    fn test_no_git_repo() {
        let tmp = TempDir::new().unwrap();
        let info = extract_git_info(tmp.path());
        assert!(!info.is_git);
        assert_eq!(info.total_commits, 0);
        assert_eq!(info.unpushed_count, 0);
    }

    #[test]
    fn test_git_repo_no_remote() {
        let tmp = TempDir::new().unwrap();
        make_repo(tmp.path(), &[1_700_000_000, 1_700_100_000]);
        let info = extract_git_info(tmp.path());
        assert!(info.is_git);
        assert!(!info.has_remote);
        assert!(!info.is_on_github);
        assert_eq!(info.total_commits, 2);
        assert_eq!(info.unpushed_count, 2); // no remote → all commits are unpushed
        assert!(info.oldest_unpushed.is_some());
        assert!(info.newest_unpushed.is_some());
        assert!(info.branches_with_unpushed.contains(&"main".to_string()));
    }

    #[test]
    fn test_git_repo_with_origin() {
        let tmp = TempDir::new().unwrap();
        let repo = make_repo(tmp.path(), &[1_700_000_000]);
        repo.remote("origin", "git@github.com:user/myrepo.git")
            .unwrap();
        let info = extract_git_info(tmp.path());
        assert!(info.is_git);
        assert!(info.has_remote);
        assert!(info.is_on_github);
        assert_eq!(
            info.origin_url,
            Some("git@github.com:user/myrepo.git".to_string())
        );
        // No remote tracking refs → all local commits still count as unpushed
        // (in practice you'd need to fetch and have remote refs, but local-only remote is fine)
    }
}
