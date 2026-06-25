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

impl Default for GitInfo {
    fn default() -> Self {
        GitInfo {
            is_git: false,
            is_worktree: false,
            has_remote: false,
            origin_url: None,
            is_on_github: false,
            unpushed_count: 0,
            oldest_unpushed: None,
            newest_unpushed: None,
            branches_with_unpushed: Vec::new(),
            total_commits: 0,
            last_modified: None,
        }
    }
}
