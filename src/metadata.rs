use serde::{Deserialize, Serialize};

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
}
