use std::path::Path;

pub struct FsInfo {
    pub has_readme: bool,
    pub has_tests: bool,
    pub has_ci: bool,
    pub has_license: bool,
}

pub fn extract_fs_info(path: &Path) -> FsInfo {
    let names: Vec<String> = std::fs::read_dir(path)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().to_lowercase())
                .collect()
        })
        .unwrap_or_default();

    let has_readme = names.iter().any(|n| n.starts_with("readme"));
    let has_license = names.iter().any(|n| n.starts_with("license"));
    let has_tests = path.join("tests").is_dir()
        || path.join("test").is_dir()
        || names
            .iter()
            .any(|n| n.ends_with("_test.rs") || n.ends_with("_spec.rb") || n == "test.py");
    let has_ci = path.join(".github").join("workflows").is_dir()
        || path.join(".travis.yml").is_file()
        || path.join(".circleci").is_dir()
        || path.join("Jenkinsfile").is_file()
        || path.join(".gitlab-ci.yml").is_file();

    FsInfo {
        has_readme,
        has_tests,
        has_ci,
        has_license,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_detects_readme() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("README.md"), "# hello").unwrap();
        let info = extract_fs_info(tmp.path());
        assert!(info.has_readme);
        assert!(!info.has_license);
    }

    #[test]
    fn test_detects_license() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("LICENSE"), "MIT").unwrap();
        let info = extract_fs_info(tmp.path());
        assert!(info.has_license);
    }

    #[test]
    fn test_detects_tests_dir() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("tests")).unwrap();
        let info = extract_fs_info(tmp.path());
        assert!(info.has_tests);
    }

    #[test]
    fn test_detects_github_ci() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".github").join("workflows")).unwrap();
        let info = extract_fs_info(tmp.path());
        assert!(info.has_ci);
    }

    #[test]
    fn test_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let info = extract_fs_info(tmp.path());
        assert!(!info.has_readme);
        assert!(!info.has_tests);
        assert!(!info.has_ci);
        assert!(!info.has_license);
    }
}
