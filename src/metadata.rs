use std::path::Path;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::fs_meta::extract_fs_info;
use crate::git_info::extract_git_info;
use crate::loc::extract_loc;
use crate::repostatus::read_repostatus;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct LanguageStat {
    pub name: String,
    pub code: u64,
    pub comments: u64,
    pub blanks: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMetadata {
    pub path: String,
    pub name: String,
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
    pub primary_language: Option<String>,
    pub languages: Vec<LanguageStat>,
    pub has_readme: bool,
    pub has_tests: bool,
    pub has_ci: bool,
    pub has_license: bool,
    pub last_modified: Option<String>,
    pub repostatus_state: String,
    pub repostatus_age_days: Option<u32>,
}

impl Default for ProjectMetadata {
    fn default() -> Self {
        ProjectMetadata {
            path: Default::default(),
            name: Default::default(),
            is_git: Default::default(),
            is_worktree: Default::default(),
            has_remote: Default::default(),
            origin_url: Default::default(),
            is_on_github: Default::default(),
            unpushed_count: Default::default(),
            oldest_unpushed: Default::default(),
            newest_unpushed: Default::default(),
            branches_with_unpushed: Default::default(),
            total_commits: Default::default(),
            primary_language: Default::default(),
            languages: Default::default(),
            has_readme: Default::default(),
            has_tests: Default::default(),
            has_ci: Default::default(),
            has_license: Default::default(),
            last_modified: Default::default(),
            repostatus_state: "unreviewed".to_string(),
            repostatus_age_days: Default::default(),
        }
    }
}

pub fn extract_metadata(path: &Path, root: &Path) -> anyhow::Result<ProjectMetadata> {
    let git = extract_git_info(path);
    let loc = extract_loc(path);
    let fs = extract_fs_info(path);
    let status = read_repostatus(path);

    let display_path = crate::simplified_repo_path(path, root);
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();

    let (repostatus_state, repostatus_age_days) = match status {
        Some(rs) => {
            let state = rs.state.unwrap_or_else(|| "unreviewed".to_string());
            let age = rs.reviewed.as_deref().and_then(|d| {
                NaiveDate::parse_from_str(d, "%Y-%m-%d").ok().map(|reviewed| {
                    let today = chrono::Local::now().date_naive();
                    (today - reviewed).num_days().max(0) as u32
                })
            });
            (state, age)
        }
        None => ("unreviewed".to_string(), None),
    };

    Ok(ProjectMetadata {
        path: display_path,
        name,
        is_git: git.is_git,
        is_worktree: git.is_worktree,
        has_remote: git.has_remote,
        origin_url: git.origin_url,
        is_on_github: git.is_on_github,
        unpushed_count: git.unpushed_count,
        oldest_unpushed: git.oldest_unpushed,
        newest_unpushed: git.newest_unpushed,
        branches_with_unpushed: git.branches_with_unpushed,
        total_commits: git.total_commits,
        primary_language: loc.primary_language,
        languages: loc.languages,
        has_readme: fs.has_readme,
        has_tests: fs.has_tests,
        has_ci: fs.has_ci,
        has_license: fs.has_license,
        last_modified: git.last_modified,
        repostatus_state,
        repostatus_age_days,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_serializes_to_json() {
        let m = ProjectMetadata {
            path: "foo/bar".to_string(),
            name: "bar".to_string(),
            is_git: true,
            repostatus_state: "unreviewed".to_string(),
            total_commits: 5,
            ..Default::default()
        };
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"path\":\"foo/bar\""));
        assert!(json.contains("\"total_commits\":5"));
        assert!(json.contains("\"repostatus_state\":\"unreviewed\""));
    }

    #[test]
    fn test_default_repostatus_state_is_unreviewed() {
        let m = ProjectMetadata::default();
        assert_eq!(m.repostatus_state, "unreviewed");
    }

    #[test]
    fn test_extract_metadata_non_git_dir() {
        let root = tempfile::TempDir::new().unwrap();
        let project = root.path().join("myproj");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(project.join("main.py"), "print('hello')\n").unwrap();

        let meta = extract_metadata(&project, root.path()).unwrap();
        assert_eq!(meta.name, "myproj");
        assert!(!meta.is_git);
        assert_eq!(meta.total_commits, 0);
        assert_eq!(meta.repostatus_state, "unreviewed");
        assert!(
            meta.primary_language.as_deref() == Some("Python"),
            "expected Python, got {:?}",
            meta.primary_language
        );
    }
}
