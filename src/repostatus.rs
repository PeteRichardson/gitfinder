use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct RepoStatus {
    pub state: Option<String>,
    pub reason: Option<String>,
    pub effort: Option<String>,
    pub reviewed: Option<String>,
    pub notes: Option<String>,
}

pub fn read_repostatus(path: &Path) -> Option<RepoStatus> {
    let content = std::fs::read_to_string(path.join(".repostatus")).ok()?;
    serde_saphyr::from_str(&content).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_reads_repostatus() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join(".repostatus"),
            "state: skip\nreason: trivial\nreviewed: 2026-01-15\n",
        )
        .unwrap();
        let rs = read_repostatus(tmp.path()).unwrap();
        assert_eq!(rs.state.as_deref(), Some("skip"));
        assert_eq!(rs.reason.as_deref(), Some("trivial"));
        assert_eq!(rs.reviewed.as_deref(), Some("2026-01-15"));
    }

    #[test]
    fn test_absent_repostatus_returns_none() {
        let tmp = TempDir::new().unwrap();
        assert!(read_repostatus(tmp.path()).is_none());
    }
}
