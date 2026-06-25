use chrono::{DateTime, Local};
use git2::{Repository, Signature, Time};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, UNIX_EPOCH};
use tempfile::TempDir;

/// Creates a git repo at `path` with one commit per entry in `commit_times`
/// (unix seconds), chained as parent -> child in the order given, all on
/// `refs/heads/main`. Each commit is created directly against that ref name
/// so the repo is discoverable regardless of the test environment's
/// `init.defaultBranch` setting.
fn init_repo_with_commits(path: &Path, commit_times: &[i64]) -> Repository {
    let repo = Repository::init(path).expect("init repo");
    {
        // Block ensures tree and parent_commit borrows are dropped before repo is returned.
        let tree_oid = repo
            .treebuilder(None)
            .expect("treebuilder")
            .write()
            .expect("write empty tree");
        let tree = repo.find_tree(tree_oid).expect("find tree");

        let mut parent_oid: Option<git2::Oid> = None;
        for &secs in commit_times {
            let sig =
                Signature::new("Test", "test@example.com", &Time::new(secs, 0)).expect("signature");
            let parent_commit =
                parent_oid.map(|oid| repo.find_commit(oid).expect("find parent commit"));
            let parents: Vec<&git2::Commit> = parent_commit.iter().collect();
            let oid = repo
                .commit(Some("refs/heads/main"), &sig, &sig, "test commit", &tree, &parents)
                .expect("commit");
            parent_oid = Some(oid);
        }
    }
    repo
}

fn format_date(secs: i64) -> String {
    let system_time = UNIX_EPOCH + Duration::from_secs(secs as u64);
    let datetime: DateTime<Local> = DateTime::from(system_time);
    datetime.format("%y-%m-%d").to_string()
}

fn run_gitfinder(dir: &Path) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_lsproj"))
        .arg(dir)
        .output()
        .expect("run gitfinder");
    assert!(output.status.success(), "gitfinder exited with failure: {:?}", output);
    String::from_utf8(output.stdout).expect("utf8 stdout")
}

#[test]
fn test_finds_repo_without_origin() {
    let root = TempDir::new().unwrap();
    let repo_dir = root.path().join("myrepo");
    std::fs::create_dir(&repo_dir).unwrap();
    let oldest = 1_700_000_000i64;
    let newest = 1_700_100_000i64;
    init_repo_with_commits(&repo_dir, &[oldest, newest]);

    let stdout = run_gitfinder(root.path());

    let expected_row = format!(
        "myrepo,{},{},2",
        format_date(oldest),
        format_date(newest)
    );
    assert!(
        stdout.lines().any(|line| line == expected_row),
        "expected row {:?} not found in output:\n{}",
        expected_row,
        stdout
    );
}

#[test]
fn test_excludes_repo_with_origin() {
    let root = TempDir::new().unwrap();
    let repo_dir = root.path().join("myrepo");
    std::fs::create_dir(&repo_dir).unwrap();
    let repo = init_repo_with_commits(&repo_dir, &[1_700_000_000]);
    repo.remote("origin", "https://example.com/myrepo.git")
        .unwrap();

    let stdout = run_gitfinder(root.path());

    assert!(
        !stdout.contains("myrepo,"),
        "repo with origin should be excluded, got:\n{}",
        stdout
    );
}

#[test]
fn test_recurses_into_nested_directories() {
    let root = TempDir::new().unwrap();
    let repo_dir = root.path().join("level1").join("level2").join("myrepo");
    std::fs::create_dir_all(&repo_dir).unwrap();
    let when = 1_700_000_000i64;
    init_repo_with_commits(&repo_dir, &[when]);

    let stdout = run_gitfinder(root.path());

    let expected_row = format!(
        "level1/level2/myrepo,{},{},1",
        format_date(when),
        format_date(when)
    );
    assert!(
        stdout.lines().any(|line| line == expected_row),
        "expected row {:?} not found in output:\n{}",
        expected_row,
        stdout
    );
}

#[test]
fn test_excludes_empty_repo() {
    let root = TempDir::new().unwrap();
    let repo_dir = root.path().join("emptyrepo");
    std::fs::create_dir(&repo_dir).unwrap();
    Repository::init(&repo_dir).unwrap();

    let stdout = run_gitfinder(root.path());

    assert!(
        !stdout.contains("emptyrepo,"),
        "empty repo should be excluded, got:\n{}",
        stdout
    );
}

#[test]
fn test_csv_header_and_date_format() {
    let root = TempDir::new().unwrap();
    let repo_dir = root.path().join("myrepo");
    std::fs::create_dir(&repo_dir).unwrap();
    init_repo_with_commits(&repo_dir, &[1_700_000_000]);

    let stdout = run_gitfinder(root.path());

    let mut lines = stdout.lines();
    assert_eq!(lines.next(), Some("repository,oldest,newest,count"));

    let date_re_ok = |s: &str| s.len() == 8 && s.chars().filter(|c| *c == '-').count() == 2;
    let row = lines.find(|l| l.starts_with("myrepo,")).expect("myrepo row present");
    let fields: Vec<&str> = row.split(',').collect();
    assert_eq!(fields.len(), 4);
    assert!(date_re_ok(fields[1]), "oldest date field malformed: {}", fields[1]);
    assert!(date_re_ok(fields[2]), "newest date field malformed: {}", fields[2]);
}
