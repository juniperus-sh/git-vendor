//! Integration tests for the `Vendor` trait methods on `Repository`.

use git_vendor::Vendor;
use git2::{Oid, Repository};
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

fn setup_upstream(files: &[(&str, &[u8])]) -> (Repository, TempDir) {
    let dir = TempDir::new().unwrap();
    let repo = Repository::init(dir.path()).unwrap();

    {
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Test").unwrap();
        config.set_str("user.email", "test@test").unwrap();
    }

    for (path, content) in files {
        let full_path = dir.path().join(path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&full_path, content).unwrap();
    }

    {
        let mut index = repo.index().unwrap();
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let oid = index.write_tree().unwrap();
        let tree = repo.find_tree(oid).unwrap();
        let sig = repo.signature().unwrap();

        repo.commit(Some("refs/heads/main"), &sig, &sig, "initial", &tree, &[])
            .unwrap();
    }
    repo.set_head("refs/heads/main").unwrap();

    (repo, dir)
}

fn write_gitattributes(dir: &Path, content: &str) {
    let path = dir.join(".gitattributes");
    let mut f = fs::File::create(&path).unwrap();
    write!(f, "{content}").unwrap();
}

/// Stage everything in the working tree and commit with the given message.
fn commit_all(repo: &Repository, message: &str) -> Oid {
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();
    let sig = repo.signature().unwrap();
    let parent = repo.head().unwrap().peel_to_commit().unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])
        .unwrap()
}

// ---------------------------------------------------------------------------
// track_pattern
// ---------------------------------------------------------------------------

#[test]
fn track_pattern_writes_gitattributes() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
    assert!(content.contains("url=https://github.com/owner/repo.git"));
    assert!(content.contains("branch=main"));
    assert!(!content.contains("prefix="));
}

#[test]
fn track_pattern_omits_branch_when_none() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (repo, dir) = setup_repo();
    std::env::set_current_dir(dir.path()).unwrap();

    repo.track_pattern("*.rs", "https://github.com/owner/repo.git", None, None)
        .unwrap();

    let content = fs::read_to_string(dir.path().join(".gitattributes")).unwrap();
    assert!(content.contains("vendored"));
    assert!(content.contains("url=https://github.com/owner/repo.git"));
    assert!(!content.contains("branch"));
}

#[test]
fn track_pattern_includes_branch_when_specified() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
    assert!(content.contains("branch=develop"));
}

// ---------------------------------------------------------------------------
// untrack_pattern
// ---------------------------------------------------------------------------

#[test]
fn untrack_pattern_removes_vendor_lines() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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

    repo.untrack_pattern("*.txt").unwrap();

    let content = fs::read_to_string(&ga).unwrap();
    assert!(!content.contains("url="));
}

#[test]
fn untrack_pattern_is_noop_without_gitattributes() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (repo, dir) = setup_repo();
    std::env::set_current_dir(dir.path()).unwrap();

    assert!(repo.untrack_pattern("*.txt").is_ok());
}

// ---------------------------------------------------------------------------
// status
// ---------------------------------------------------------------------------

#[test]
fn status_ok_with_no_deps() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (repo, dir) = setup_repo();
    std::env::set_current_dir(dir.path()).unwrap();

    assert!(repo.vendor_status(None).is_ok());
}

#[test]
fn status_ok_with_tracked_dep() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (repo, dir) = setup_repo();
    std::env::set_current_dir(dir.path()).unwrap();

    write_gitattributes(
        dir.path(),
        "*.txt vendored url=https://example.com/o/r.git branch=main\n",
    );

    assert!(repo.vendor_status(None).is_ok());
}

#[test]
fn status_ok_with_tracked_dep_no_branch() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (repo, dir) = setup_repo();
    std::env::set_current_dir(dir.path()).unwrap();

    write_gitattributes(
        dir.path(),
        "*.txt vendored url=https://example.com/o/r.git\n",
    );

    assert!(repo.vendor_status(None).is_ok());
}

// ---------------------------------------------------------------------------
// fetch
// ---------------------------------------------------------------------------

#[test]
fn fetch_errors_with_no_deps() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (repo, dir) = setup_repo();
    std::env::set_current_dir(dir.path()).unwrap();

    let err = Vendor::vendor_merge(&repo, None, Some(&Default::default())).unwrap_err();
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
    assert!(Vendor::vendor_merge(&repo, None, Some(&Default::default())).is_err());
}

// ---------------------------------------------------------------------------
// merge preserves non-vendor files
// ---------------------------------------------------------------------------

#[test]
fn merge_preserves_non_vendor_files() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    // 1. Create an upstream (vendor source) repo with a file that matches
    //    the vendor pattern.
    let (_upstream_repo, upstream_dir) = setup_upstream(&[("lib.txt", b"vendored content\n")]);

    // 2. Set up the host repo with a non-vendor file committed to HEAD.
    let (repo, dir) = setup_repo();
    std::env::set_current_dir(dir.path()).unwrap();

    fs::write(dir.path().join("README.md"), "# My Project\n").unwrap();
    write_gitattributes(
        dir.path(),
        &format!(
            "*.txt vendored url={} branch=main\n",
            upstream_dir.path().display(),
        ),
    );
    commit_all(&repo, "add README and vendor config");

    // Sanity: README.md is in HEAD before the merge.
    let head_tree = repo.head().unwrap().peel_to_tree().unwrap();
    assert!(
        head_tree.get_name("README.md").is_some(),
        "README.md should exist in HEAD before merge"
    );

    // 3. Fetch + merge the vendor dependency.
    repo.vendor_fetch(None, None).unwrap();
    repo.vendor_merge(None, Some(&Default::default())).unwrap();

    // 4. Non-vendor files must still be present in HEAD and working tree.
    let head_tree = repo.head().unwrap().peel_to_tree().unwrap();
    assert!(
        head_tree.get_name("README.md").is_some(),
        "README.md must survive the vendor merge in the commit tree"
    );
    assert!(
        head_tree.get_name(".gitattributes").is_some(),
        ".gitattributes must survive the vendor merge in the commit tree"
    );
    assert!(
        dir.path().join("README.md").exists(),
        "README.md must survive the vendor merge in the working tree"
    );

    // 5. Vendor content must have been merged in.
    assert!(
        head_tree.get_name("lib.txt").is_some(),
        "vendor file lib.txt should be present after merge"
    );
    assert!(
        dir.path().join("lib.txt").exists(),
        "vendor file lib.txt should be in the working tree after merge"
    );
}

// ---------------------------------------------------------------------------
// merge rejects dirty index
// ---------------------------------------------------------------------------

#[test]
fn merge_rejects_dirty_index() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let (_upstream_repo, upstream_dir) = setup_upstream(&[("lib.txt", b"content\n")]);

    let (repo, dir) = setup_repo();
    std::env::set_current_dir(dir.path()).unwrap();

    write_gitattributes(
        dir.path(),
        &format!(
            "*.txt vendored url={} branch=main\n",
            upstream_dir.path().display(),
        ),
    );
    commit_all(&repo, "vendor config");

    repo.vendor_fetch(None, None).unwrap();

    // Stage a new file without committing — the index is now dirty.
    fs::write(dir.path().join("staged.txt"), "uncommitted\n").unwrap();
    {
        let mut index = repo.index().unwrap();
        index
            .add_all(["staged.txt"].iter(), git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
    }

    let err = repo
        .vendor_merge(None, Some(&Default::default()))
        .unwrap_err();
    assert!(
        err.message().contains("uncommitted changes"),
        "expected dirty-index error, got: {}",
        err.message()
    );
}

// ---------------------------------------------------------------------------
// merge places vendor content at the pattern path
// ---------------------------------------------------------------------------

#[test]
fn merge_vendors_subdirectory_from_upstream() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    // The upstream repo itself contains a `pyo3/` directory.  The pattern
    // `pyo3/**` filters the vendor tree to that subtree, and it lands in the
    // host repo at the same path: pyo3/ → pyo3/.
    let (_upstream_repo, upstream_dir) = setup_upstream(&[
        ("pyo3/Cargo.toml", b"[package]\nname = \"pyo3\"\n"),
        ("pyo3/README.md", b"# pyo3\n"),
        ("pyo3/src/lib.rs", b"pub fn hello() {}\n"),
        ("pyo3/src/util.rs", b"pub fn helper() {}\n"),
        ("other/unrelated.txt", b"not vendored\n"),
    ]);

    let (repo, dir) = setup_repo();
    std::env::set_current_dir(dir.path()).unwrap();

    // Host has its own top-level file.
    fs::write(dir.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    write_gitattributes(
        dir.path(),
        &format!(
            "pyo3/** vendored url={} branch=main\n",
            upstream_dir.path().display(),
        ),
    );
    commit_all(&repo, "initial");

    repo.vendor_fetch(None, None).unwrap();
    repo.vendor_merge(None, Some(&Default::default())).unwrap();

    let head_tree = repo.head().unwrap().peel_to_tree().unwrap();

    // Host's own files must survive.
    assert!(
        head_tree.get_name("Cargo.toml").is_some(),
        "host Cargo.toml must survive the merge"
    );

    // Vendor content must appear under `pyo3/`.
    assert!(
        head_tree.get_name("pyo3").is_some(),
        "pyo3/ directory should exist after merge"
    );
    assert!(
        dir.path().join("pyo3").join("Cargo.toml").exists(),
        "pyo3/Cargo.toml should be in the working tree"
    );
    assert!(
        dir.path().join("pyo3").join("README.md").exists(),
        "pyo3/README.md should be in the working tree"
    );

    // Nested subdirectories from the vendor must also be present.
    assert!(
        dir.path().join("pyo3").join("src").join("lib.rs").exists(),
        "pyo3/src/lib.rs should be in the working tree"
    );
    assert!(
        dir.path().join("pyo3").join("src").join("util.rs").exists(),
        "pyo3/src/util.rs should be in the working tree"
    );

    // Content outside the pattern must NOT appear.
    assert!(
        head_tree.get_name("other").is_none(),
        "other/ from vendor must not appear in the host tree"
    );
}

// ---------------------------------------------------------------------------
// trailing-slash pattern (e.g. "pyo3/") must behave like "pyo3/**"
// ---------------------------------------------------------------------------

#[test]
fn merge_vendors_subdirectory_trailing_slash_pattern() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    // Upstream has pyo3/ with nested content, plus unrelated files.
    let (_upstream_repo, upstream_dir) = setup_upstream(&[
        ("pyo3/Cargo.toml", b"[package]\nname = \"pyo3\"\n"),
        ("pyo3/src/lib.rs", b"pub fn hello() {}\n"),
        ("other/unrelated.txt", b"not vendored\n"),
    ]);

    let (repo, dir) = setup_repo();
    std::env::set_current_dir(dir.path()).unwrap();

    fs::write(dir.path().join("README.md"), "# host\n").unwrap();
    // Use "pyo3/" (trailing slash, no glob stars) — the pattern a user
    // would naturally type for a directory.
    write_gitattributes(
        dir.path(),
        &format!(
            "pyo3/ vendored url={} branch=main\n",
            upstream_dir.path().display(),
        ),
    );
    commit_all(&repo, "initial");

    repo.vendor_fetch(None, None).unwrap();
    repo.vendor_merge(None, Some(&Default::default())).unwrap();

    let head_tree = repo.head().unwrap().peel_to_tree().unwrap();

    // Host files must survive.
    assert!(
        head_tree.get_name("README.md").is_some(),
        "host README.md must survive the merge"
    );

    // Vendor content must appear under pyo3/.
    assert!(
        head_tree.get_name("pyo3").is_some(),
        "pyo3/ directory should exist after merge"
    );
    assert!(
        dir.path().join("pyo3").join("Cargo.toml").exists(),
        "pyo3/Cargo.toml should be in the working tree"
    );
    assert!(
        dir.path().join("pyo3").join("src").join("lib.rs").exists(),
        "pyo3/src/lib.rs should be in the working tree"
    );

    // Content outside the pattern must NOT appear.
    assert!(
        head_tree.get_name("other").is_none(),
        "other/ from vendor must not appear in the host tree"
    );
}
