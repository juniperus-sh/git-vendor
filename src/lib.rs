//! In-source vendoring for Git repositories.
//!
//! Vendor dependencies are tracked via custom attributes in `.gitattributes`:
//!
//! ```text
//! path/to/dep/* vendored name=owner/repo url=https://example.com/owner/repo.git branch=main
//! ```

use git_filter_tree::FilterTree;
use git_set_attr::SetAttr;
use git2::{Error, FetchOptions, MergeOptions, Repository};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

/// A vendored dependency parsed from `.gitattributes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VendorDep {
    pub pattern: String,
    pub url: String,
    pub reference: Option<String>,
    pub prefix: Option<String>,
}

pub trait Vendor {
    /// Add the pattern to the appropriate `.gitattributes` file using `git_set_attr`.
    ///
    /// If there is a `.gitattributes` file in the current directory, that file is used.
    /// Otherwise, the first found `.gitattributes` file when walking up the directory
    /// tree from the current directory to the repository root directory is used.
    ///
    /// If the pattern is already specified, the `url`, `branch`, and `prefix` are updated if necessary.
    fn track_pattern(
        &self,
        pattern: &str,
        url: &str,
        maybe_reference: Option<&str>,
        maybe_prefix: Option<&str>,
    ) -> Result<(), Error>;

    /// Remove the pattern from the appropriate `.gitattributes` file using `git_set_attr`.
    ///
    /// If there is a `.gitattributes` file in the current directory, that file is used.
    /// Otherwise, the first found `.gitattributes` file when walking up the directory
    /// tree from the current directory to the repository root directory is used.
    fn untrack_pattern(&self, pattern: &str) -> Result<(), Error>;

    /// Return the status of all vendored content, or any errors encountered along the way.
    fn vendor_status(&self, maybe_pattern: Option<&str>) -> Result<&[VendorDep], Error>;

    /// Fetch the latest content from all relevant vendor sources.
    fn vendor_fetch(
        &self,
        maybe_pattern: Option<&str>,
        fetch_opts: Option<&mut FetchOptions<'_>>,
    ) -> Result<(), Error>;

    /// Merge the latest content from all relevant vendor sources.
    ///
    /// Behaves like `git merge`: updates the working tree and index, optionally
    /// creates a merge commit, and records `MERGE_HEAD`/`MERGE_MSG` when
    /// appropriate.
    fn vendor_merge(
        &self,
        maybe_pattern: Option<&str>,
        merge_opts: Option<&MergeOptions>,
    ) -> Result<(), Error>;
}

impl Vendor for Repository {
    fn track_pattern(
        &self,
        pattern: &str,
        url: &str,
        maybe_reference: Option<&str>,
        maybe_prefix: Option<&str>,
    ) -> Result<(), Error> {
        require_non_bare(self)?;

        let url_attr = format!("url={url}");
        let prefix_attr = maybe_prefix.map(|prefix| format!("prefix={prefix}"));
        let branch_attr = maybe_reference.map(|branch| format!("branch={branch}"));

        let mut attrs = vec!["vendored", &url_attr];

        if let Some(ref prefix) = prefix_attr {
            attrs.push(prefix);
        }

        if let Some(ref branch) = branch_attr {
            attrs.push(branch);
        }

        self.set_attr(pattern, &attrs, None)
    }

    fn untrack_pattern(&self, pattern: &str) -> Result<(), Error> {
        require_non_bare(self)?;

        let path = find_gitattributes(self)?;
        if !path.exists() {
            return Ok(());
        }

        remove_vendor_lines(&path, pattern)
    }

    fn vendor_status(&self, maybe_pattern: Option<&str>) -> Result<&[VendorDep], Error> {
        require_non_bare(self)?;

        let path = find_gitattributes(self)?;
        let deps = {
            let unfiltered_deps = parse_vendor_deps(&path)?;
            filter_deps(&unfiltered_deps, maybe_pattern);
        };

        todo!();
    }

    fn vendor_fetch(
        &self,
        maybe_pattern: Option<&str>,
        mut maybe_opts: Option<&mut FetchOptions<'_>>,
    ) -> Result<(), Error> {
        require_non_bare(self)?;

        let path = find_gitattributes(self)?;
        let deps = parse_vendor_deps(&path)?;
        let deps = filter_deps(&deps, maybe_pattern);

        if deps.is_empty() {
            return Err(Error::from_str("No vendored dependencies to fetch"));
        }

        todo!();

        Ok(())
    }

    fn vendor_merge(
        &self,
        maybe_pattern: Option<&str>,
        merge_opts: Option<&MergeOptions>,
    ) -> Result<(), Error> {
        todo!();
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Merge state helpers
// ---------------------------------------------------------------------------

/// Write `MERGE_MSG` so that `git commit` picks up the message.
fn set_merge_msg(repo: &Repository, msg: &str) -> Result<(), Error> {
    let path = repo.path().join("MERGE_MSG");
    fs::write(&path, format!("{msg}\n")).map_err(|e| Error::from_str(&e.to_string()))
}

// ---------------------------------------------------------------------------
// Repository helpers
// ---------------------------------------------------------------------------

fn require_non_bare(repo: &Repository) -> Result<(), Error> {
    if repo.is_bare() {
        Err(Error::from_str(
            "This operation is not supported in a bare repository",
        ))
    } else {
        Ok(())
    }
}

/// Return `true` if `url` looks like a remote URL rather than a local path.
///
/// Recognizes `scheme://...` and SCP-style `user@host:path`.
fn is_remote_url(url: &str) -> bool {
    // scheme://...
    if url.contains("://") {
        return true;
    }
    // SCP-style: git@host:path  (must have @ before : and no path separators before @)
    if let Some(at) = url.find('@')
        && let Some(colon) = url[at..].find(':')
    {
        let colon_pos = at + colon;
        // Make sure the part before @ has no slashes (not a path)
        if !url[..at].contains('/') && colon_pos + 1 < url.len() {
            return true;
        }
    }
    false
}

/// Find the appropriate `.gitattributes` file by walking from the current
/// directory up to the repository root.
///
/// Returns the path of the first `.gitattributes` file found, or defaults to
/// `<current_dir>/.gitattributes` (which will be created on first write).
fn find_gitattributes(repo: &Repository) -> Result<PathBuf, Error> {
    let workdir = repo
        .workdir()
        .ok_or_else(|| Error::from_str("Repository has no working directory"))?;

    let current_dir = std::env::current_dir()
        .map_err(|e| Error::from_str(&format!("Failed to get current directory: {e}")))?;

    let mut dir = current_dir.as_path();
    while dir.starts_with(workdir) {
        let candidate = dir.join(".gitattributes");
        if candidate.exists() {
            return Ok(candidate);
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => break,
        }
    }

    Ok(current_dir.join(".gitattributes"))
}

/// Parse vendor dependencies from a `.gitattributes` file.
///
/// A line is recognized as a vendor dependency when it carries at least
/// `name=` and `url=`. The `branch=` attribute is
/// optional — when absent, the dependency tracks the remote's default branch.
fn parse_vendor_deps(path: &Path) -> Result<Vec<VendorDep>, Error> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = fs::File::open(path)
        .map_err(|e| Error::from_str(&format!("Failed to open {}: {e}", path.display())))?;

    let mut deps = Vec::new();

    for line in BufReader::new(file).lines() {
        let line =
            line.map_err(|e| Error::from_str(&format!("Failed to read .gitattributes: {e}")))?;
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let mut parts = trimmed.split_whitespace();
        let pattern = match parts.next() {
            Some(p) => p,
            None => continue,
        };

        let mut url = None;
        let mut branch = None;
        let mut prefix = None;
        let mut is_vendored = false;

        for attr in parts {
            if attr == "vendored" {
                is_vendored = true;
            } else if let Some(v) = attr.strip_prefix("url=") {
                url = Some(v.to_string());
            } else if let Some(v) = attr.strip_prefix("branch=") {
                branch = Some(v.to_string());
            } else if let Some(v) = attr.strip_prefix("prefix=") {
                prefix = Some(v.to_string());
            }
        }

        if !is_vendored {
            continue;
        }

        if let Some(url) = url {
            deps.push(VendorDep {
                pattern: pattern.to_string(),
                url,
                reference: branch,
                prefix,
            });
        }
    }

    Ok(deps)
}

/// Remove all lines from a `.gitattributes` file that match `pattern` **and**
/// carry vendor attributes.  Non-vendor lines for the same pattern are kept.
fn remove_vendor_lines(path: &Path, pattern: &str) -> Result<(), Error> {
    if !path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(path)
        .map_err(|e| Error::from_str(&format!("Failed to read {}: {e}", path.display())))?;

    let mut kept = Vec::new();
    for line in content.lines() {
        if is_vendor_line_for_pattern(line, pattern) {
            // FIXME: what if other non-vendor-related attributes are on this line?
            continue;
        }
        kept.push(line);
    }

    let mut file = fs::File::create(path)
        .map_err(|e| Error::from_str(&format!("Failed to write {}: {e}", path.display())))?;

    for line in &kept {
        writeln!(file, "{line}")
            .map_err(|e| Error::from_str(&format!("Failed to write .gitattributes: {e}")))?;
    }

    file.flush()
        .map_err(|e| Error::from_str(&format!("Failed to flush .gitattributes: {e}")))?;

    Ok(())
}

/// Return `true` if `line` starts with `pattern` and contains at least one
/// vendor attribute (`vendored`, `name=`, `url=`, or
/// `branch=`).
fn is_vendor_line_for_pattern(line: &str, pattern: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return false;
    }

    let mut parts = trimmed.split_whitespace();
    let line_pattern = match parts.next() {
        Some(p) => p,
        None => return false,
    };

    if line_pattern != pattern {
        return false;
    }

    parts.any(|attr| {
        attr == "vendored"
            || attr.starts_with("name=")
            || attr.starts_with("url=")
            || attr.starts_with("branch=")
    })
}

/// Filter dependencies by exact pattern match.
fn filter_deps<'a>(deps: &'a [VendorDep], filter: Option<&str>) -> Vec<&'a VendorDep> {
    match filter {
        None => deps.iter().collect(),
        Some(f) => deps.iter().filter(|d| d.pattern == f).collect(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    use tempfile::TempDir;

    // -- is_remote_url ------------------------------------------------------

    #[test]
    fn is_remote_url_https() {
        assert!(is_remote_url("https://github.com/owner/repo.git"));
    }

    #[test]
    fn is_remote_url_ssh_scheme() {
        assert!(is_remote_url("ssh://git@github.com/owner/repo.git"));
    }

    #[test]
    fn is_remote_url_scp_style() {
        assert!(is_remote_url("git@github.com:owner/repo.git"));
    }

    #[test]
    fn is_remote_url_rejects_absolute_path() {
        assert!(!is_remote_url("/home/user/repos/mylib"));
    }

    #[test]
    fn is_remote_url_rejects_relative_path() {
        assert!(!is_remote_url("../repos/mylib"));
    }

    // -- name_from_url ------------------------------------------------------

    #[test]
    #[test]
    #[test]
    #[test]
    // -- resolve_name -------------------------------------------------------
    #[test]
    #[test]
    // -- vendor_ref_name ----------------------------------------------------
    #[test]
    // -- parse_vendor_deps --------------------------------------------------
    #[test]
    fn parse_vendor_deps_from_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".gitattributes");

        let mut f = fs::File::create(&path).unwrap();
        writeln!(
            f,
            "*.txt vendored name=o/r1 url=https://a.com/o/r1.git branch=main"
        )
        .unwrap();
        writeln!(
            f,
            "*.rs vendored name=o/r2 url=https://b.com/o/r2.git branch=dev"
        )
        .unwrap();
        writeln!(f, "*.toml vendored name=o/r3 url=https://c.com/o/r3.git").unwrap();
        writeln!(f, "# comment").unwrap();
        writeln!(f, "*.md diff").unwrap();
        writeln!(f).unwrap();
        drop(f);

        let deps = parse_vendor_deps(&path).unwrap();
        assert_eq!(deps.len(), 3);

        assert_eq!(deps[0].pattern, "*.txt");
        assert_eq!(deps[0].url, "https://a.com/o/r1.git");
        assert_eq!(deps[0].reference, Some("main".into()));

        assert_eq!(deps[1].pattern, "*.rs");
        assert_eq!(deps[1].url, "https://b.com/o/r2.git");
        assert_eq!(deps[1].reference, Some("dev".into()));

        assert_eq!(deps[2].pattern, "*.toml");
        assert_eq!(deps[2].url, "https://c.com/o/r3.git");
        assert_eq!(deps[2].reference, None);
    }

    #[test]
    fn parse_vendor_deps_missing_file_returns_empty() {
        let deps = parse_vendor_deps(Path::new("/nonexistent/.gitattributes")).unwrap();
        assert!(deps.is_empty());
    }

    #[test]
    fn parse_vendor_deps_skips_lines_missing_any_required_vendor_attr() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".gitattributes");

        // Missing name → skip
        fs::write(&path, "*.txt url=https://a.com/o/r.git branch=main\n").unwrap();
        assert!(parse_vendor_deps(&path).unwrap().is_empty());

        // Missing url → skip
        fs::write(&path, "*.txt name=o/r branch=main\n").unwrap();
        assert!(parse_vendor_deps(&path).unwrap().is_empty());
    }

    #[test]
    fn parse_vendor_deps_branch_is_optional() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".gitattributes");

        // Missing branch → still parsed, branch is None
        fs::write(&path, "*.txt vendored name=o/r url=https://a.com/o/r.git\n").unwrap();
        let deps = parse_vendor_deps(&path).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].reference, None);
    }

    // -- is_vendor_line_for_pattern -----------------------------------------

    #[test]
    fn is_vendor_line_matches() {
        assert!(is_vendor_line_for_pattern(
            "*.txt vendored name=o/r url=https://a.com branch=main",
            "*.txt"
        ));
    }

    #[test]
    fn is_vendor_line_matches_vendored_only() {
        assert!(is_vendor_line_for_pattern("*.txt vendored", "*.txt"));
    }

    #[test]
    fn is_vendor_line_ignores_other_patterns() {
        assert!(!is_vendor_line_for_pattern(
            "*.rs vendored name=o/r url=https://a.com branch=main",
            "*.txt"
        ));
    }

    #[test]
    fn is_vendor_line_ignores_non_vendor_lines() {
        assert!(!is_vendor_line_for_pattern("*.txt diff -text", "*.txt"));
    }

    #[test]
    fn is_vendor_line_ignores_comments_and_blanks() {
        assert!(!is_vendor_line_for_pattern("# comment", "*.txt"));
        assert!(!is_vendor_line_for_pattern("", "*.txt"));
        assert!(!is_vendor_line_for_pattern("   ", "*.txt"));
    }

    // -- remove_vendor_lines ------------------------------------------------

    #[test]
    fn remove_vendor_lines_keeps_non_vendor() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".gitattributes");

        let original = "\
*.txt vendored name=o/r url=https://a.com branch=main
*.txt diff
*.rs vendored name=x/y url=https://b.com branch=dev
# comment
";
        fs::write(&path, original).unwrap();

        remove_vendor_lines(&path, "*.txt").unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(!content.contains("url=https://a.com"));
        assert!(content.contains("*.txt diff"));
        assert!(content.contains("*.rs vendored name=x/y"));
        assert!(content.contains("# comment"));
    }

    #[test]
    fn remove_vendor_lines_noop_for_missing_file() {
        assert!(remove_vendor_lines(Path::new("/nonexistent/.gitattributes"), "*.txt").is_ok());
    }

    // -- filter_deps --------------------------------------------------------

    #[test]
    fn filter_deps_none_returns_all() {
        let deps = vec![
            VendorDep {
                prefix: Some("a/b".into()),
                pattern: "a".into(),
                url: "u".into(),
                reference: Some("b".into()),
            },
            VendorDep {
                prefix: Some("c/d".into()),
                pattern: "b".into(),
                url: "u".into(),
                reference: None,
            },
        ];
        assert_eq!(filter_deps(&deps, None).len(), 2);
    }

    #[test]
    fn filter_deps_exact_match() {
        let deps = vec![
            VendorDep {
                prefix: Some("a/b".into()),
                pattern: "*.txt".into(),
                url: "u".into(),
                reference: Some("b".into()),
            },
            VendorDep {
                prefix: Some("c/d".into()),
                pattern: "*.rs".into(),
                url: "u".into(),
                reference: None,
            },
        ];
        let filtered = filter_deps(&deps, Some("*.txt"));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].pattern, "*.txt");
    }

    #[test]
    fn filter_deps_no_match() {
        let deps = vec![VendorDep {
            prefix: None,
            pattern: "*.txt".into(),
            url: "u".into(),
            reference: Some("b".into()),
        }];
        assert!(filter_deps(&deps, Some("*.rs")).is_empty());
    }
}
