//! Integration tests for the `Vendor` trait methods on `Repository`.

use git_vendor::{Vendor, VendorMergeOpts};
use git2::Repository;
use std::{fs, io::Write, path::Path, sync::Mutex};
use tempfile::TempDir;

/// Mutex to serialize tests that call `std::env::set_current_dir`, since
/// the current directory is process-global state.
static CWD_LOCK: Mutex<()> = Mutex::new(());

fn setup_repo() -> (Repository, TempDir) {
    let dir = TempDir::new().unwrap();
    let repo = Repository::init(dir.path()).unwrap();

    let mut config = repo.config().unwrap();
    config.set_str("user.name", "Test").unwrap();
    config.set_str("user.email", "test@test").unwrap();

    // Create an initial empty commit so HEAD exists.
    let sig = repo.signature().unwrap();
    let oid = {
        let mut idx = repo.index().unwrap();
        idx.write_tree().unwrap()
    };
    {
        let tree = repo.find_tree(oid).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
    }

    (repo, dir)
}

fn write_gitattributes(dir: &Path, content: &str) {
    let path = dir.join(".gitattributes");
    let mut f = fs::File::create(&path).unwrap();
    write!(f, "{content}").unwrap();
}

// ---------------------------------------------------------------------------
// track_pattern
// ---------------------------------------------------------------------------

#[test]
fn track_pattern_writes_gitattributes() {
    let _guard = CWD_LOCK.lock().unwrap();
    let (repo, dir) = setup_repo();
    std::env::set_current_dir(dir.path()).unwrap();

    repo.track_pattern(
        "*.txt",
        "https://github.com/owner/repo.git",
        Some("main"),
        None,
    )
    .unwrap();

    let content = fs::read_to_string(dir.path().join(".gitattributes")).unwrap();
    assert!(content.contains("*.txt"));
    assert!(content.contains("vendored"));
    assert!(content.contains("vendor-name=owner/repo"));
    assert!(content.contains("vendor-url=https://github.com/owner/repo.git"));
    assert!(content.contains("vendor-branch=main"));
}

#[test]
fn track_pattern_omits_branch_when_none() {
    let _guard = CWD_LOCK.lock().unwrap();
    let (repo, dir) = setup_repo();
    std::env::set_current_dir(dir.path()).unwrap();

    repo.track_pattern("*.rs", "https://github.com/owner/repo.git", None, None)
        .unwrap();

    let content = fs::read_to_string(dir.path().join(".gitattributes")).unwrap();
    assert!(content.contains("vendored"));
    assert!(content.contains("vendor-name=owner/repo"));
    assert!(content.contains("vendor-url=https://github.com/owner/repo.git"));
    assert!(!content.contains("vendor-branch"));
}

#[test]
fn track_pattern_local_path_requires_name() {
    let _guard = CWD_LOCK.lock().unwrap();
    let (repo, dir) = setup_repo();
    std::env::set_current_dir(dir.path()).unwrap();

    assert!(
        repo.track_pattern("*.txt", "/local/path", Some("main"), None)
            .is_err()
    );
    assert!(
        repo.track_pattern("*.txt", "/local/path", Some("main"), Some("my-dep"))
            .is_ok()
    );
}

#[test]
fn track_pattern_explicit_name_overrides_derived() {
    let _guard = CWD_LOCK.lock().unwrap();
    let (repo, dir) = setup_repo();
    std::env::set_current_dir(dir.path()).unwrap();

    repo.track_pattern(
        "*.txt",
        "https://github.com/owner/repo.git",
        Some("main"),
        Some("custom-name"),
    )
    .unwrap();

    let content = fs::read_to_string(dir.path().join(".gitattributes")).unwrap();
    assert!(content.contains("vendor-name=custom-name"));
}

#[test]
fn track_pattern_includes_branch_when_specified() {
    let _guard = CWD_LOCK.lock().unwrap();
    let (repo, dir) = setup_repo();
    std::env::set_current_dir(dir.path()).unwrap();

    repo.track_pattern(
        "*.rs",
        "https://github.com/owner/repo.git",
        Some("develop"),
        None,
    )
    .unwrap();

    let content = fs::read_to_string(dir.path().join(".gitattributes")).unwrap();
    assert!(content.contains("vendor-branch=develop"));
}

// ---------------------------------------------------------------------------
// untrack_pattern
// ---------------------------------------------------------------------------

#[test]
fn untrack_pattern_removes_vendor_lines() {
    let _guard = CWD_LOCK.lock().unwrap();
    let (repo, dir) = setup_repo();
    std::env::set_current_dir(dir.path()).unwrap();

    repo.track_pattern(
        "*.txt",
        "https://github.com/owner/repo.git",
        Some("main"),
        None,
    )
    .unwrap();

    let ga = dir.path().join(".gitattributes");
    let content = fs::read_to_string(&ga).unwrap();
    assert!(content.contains("vendor-name=owner/repo"));

    repo.untrack_pattern("*.txt").unwrap();

    let content = fs::read_to_string(&ga).unwrap();
    assert!(!content.contains("vendor-name=owner/repo"));
    assert!(!content.contains("vendor-url="));
}

#[test]
fn untrack_pattern_is_noop_without_gitattributes() {
    let _guard = CWD_LOCK.lock().unwrap();
    let (repo, dir) = setup_repo();
    std::env::set_current_dir(dir.path()).unwrap();

    assert!(repo.untrack_pattern("*.txt").is_ok());
}

// ---------------------------------------------------------------------------
// status
// ---------------------------------------------------------------------------

#[test]
fn status_ok_with_no_deps() {
    let _guard = CWD_LOCK.lock().unwrap();
    let (repo, dir) = setup_repo();
    std::env::set_current_dir(dir.path()).unwrap();

    assert!(repo.vendor_status(None).is_ok());
}

#[test]
fn status_ok_with_tracked_dep() {
    let _guard = CWD_LOCK.lock().unwrap();
    let (repo, dir) = setup_repo();
    std::env::set_current_dir(dir.path()).unwrap();

    write_gitattributes(
        dir.path(),
        "*.txt vendored vendor-name=o/r vendor-url=https://example.com/o/r.git vendor-branch=main\n",
    );

    assert!(repo.vendor_status(None).is_ok());
}

#[test]
fn status_ok_with_tracked_dep_no_branch() {
    let _guard = CWD_LOCK.lock().unwrap();
    let (repo, dir) = setup_repo();
    std::env::set_current_dir(dir.path()).unwrap();

    write_gitattributes(
        dir.path(),
        "*.txt vendored vendor-name=o/r vendor-url=https://example.com/o/r.git\n",
    );

    assert!(repo.vendor_status(None).is_ok());
}

// ---------------------------------------------------------------------------
// fetch
// ---------------------------------------------------------------------------

#[test]
fn fetch_errors_with_no_deps() {
    let _guard = CWD_LOCK.lock().unwrap();
    let (repo, dir) = setup_repo();
    std::env::set_current_dir(dir.path()).unwrap();

    let err = repo.vendor_fetch(None, None).unwrap_err();
    assert!(err.message().contains("No vendored dependencies to fetch"));
}

// ---------------------------------------------------------------------------
// merge
// ---------------------------------------------------------------------------

#[test]
fn merge_errors_with_no_deps() {
    let _guard = CWD_LOCK.lock().unwrap();
    let (repo, dir) = setup_repo();
    std::env::set_current_dir(dir.path()).unwrap();

    let err = Vendor::vendor_merge(&repo, None, &Default::default(), None).unwrap_err();
    assert!(err.message().contains("No vendored dependencies to merge"));
}

// ---------------------------------------------------------------------------
// bare repository
// ---------------------------------------------------------------------------

#[test]
fn bare_repo_rejects_all_operations() {
    let dir = TempDir::new().unwrap();
    let repo = Repository::init_bare(dir.path()).unwrap();

    assert!(
        repo.track_pattern("*.txt", "https://github.com/o/r.git", None, None)
            .is_err()
    );
    assert!(repo.untrack_pattern("*.txt").is_err());
    assert!(repo.vendor_status(None).is_err());
    assert!(repo.vendor_fetch(None, None).is_err());
    assert!(Vendor::vendor_merge(&repo, None, &VendorMergeOpts::default(), None).is_err());
}
