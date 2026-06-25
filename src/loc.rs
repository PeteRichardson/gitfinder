use crate::metadata::LanguageStat;
pub struct LocInfo {
    pub languages: Vec<LanguageStat>,
    pub primary_language: Option<String>,
}
