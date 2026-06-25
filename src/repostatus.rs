use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct RepoStatus {
    pub state: Option<String>,
    pub reason: Option<String>,
    pub effort: Option<String>,
    pub reviewed: Option<String>,
    pub notes: Option<String>,
}
