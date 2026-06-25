use std::cmp::Reverse;
use std::path::Path;
use tokei::{Config, Languages};

use crate::metadata::LanguageStat;

pub struct LocInfo {
    pub languages: Vec<LanguageStat>,
    pub primary_language: Option<String>,
}

pub fn extract_loc(path: &Path) -> LocInfo {
    let config = Config::default();
    let mut languages = Languages::new();
    languages.get_statistics(&[path], &[], &config);

    let mut stats: Vec<LanguageStat> = languages
        .iter()
        .filter(|(_, lang)| lang.code > 0 || lang.comments > 0 || lang.blanks > 0)
        .map(|(lang_type, lang)| LanguageStat {
            name: lang_type.to_string(),
            code: lang.code as u64,
            comments: lang.comments as u64,
            blanks: lang.blanks as u64,
        })
        .collect();

    stats.sort_by_key(|s| Reverse(s.code));

    let primary_language = stats.first().map(|s| s.name.clone());

    LocInfo {
        languages: stats,
        primary_language,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_loc_rust_file() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("main.rs"),
            "fn main() {\n    println!(\"hi\");\n}\n",
        )
        .unwrap();
        let info = extract_loc(tmp.path());
        // tokei should detect Rust
        assert!(
            info.primary_language.as_deref() == Some("Rust"),
            "expected Rust, got {:?}",
            info.primary_language
        );
        assert!(!info.languages.is_empty());
        let rust = info.languages.iter().find(|l| l.name == "Rust").unwrap();
        assert!(rust.code > 0);
    }

    #[test]
    fn test_loc_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let info = extract_loc(tmp.path());
        assert!(info.languages.is_empty());
        assert!(info.primary_language.is_none());
    }
}
