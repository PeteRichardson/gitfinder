use std::path::Path;

use chrono::Local;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct RepoStatus {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reviewed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

pub fn write_repostatus(path: &Path, state: &str, reason: Option<&str>) -> anyhow::Result<()> {
    let file_path = path.join(".repostatus");
    let today = Local::now().format("%Y-%m-%d").to_string();

    // Read existing file to preserve notes and other fields
    let mut existing = read_repostatus(path).unwrap_or_default();

    existing.state = Some(state.to_string());
    existing.reason = reason.map(|s| s.to_string());
    existing.reviewed = Some(today);

    let content = serde_saphyr::to_string(&existing)?;
    std::fs::write(&file_path, content)?;
    Ok(())
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
