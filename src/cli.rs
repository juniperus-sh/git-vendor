use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "git-vendor")]
#[command(author, version, about = "In-source vendoring alternative to Git submodules and subtrees", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Track a new vendored dependency pattern
    Track {
        /// Gitattributes-style pattern (e.g. "vendor/lib/*", "deps/*/")
        pattern: String,

        /// Remote URL or path to the dependency repository
        url: String,

        /// Branch to track (optional)
        #[arg(short, long)]
        branch: Option<String>,

        /// Prefix to store vendored files under
        #[arg(short, long)]
        prefix: Option<String>,
    },

    /// Untrack a vendored dependency pattern
    Untrack {
        /// Gitattributes-style pattern to untrack
        pattern: String,
    },

    /// Show status of vendored dependencies
    Status {
        /// Optional pattern to filter status output
        pattern: Option<String>,
    },

    /// Fetch latest content from vendored dependency sources
    Fetch {
        /// Optional pattern to filter which dependencies to fetch
        pattern: Option<String>,
    },

    /// Merge latest content from vendored dependency sources
    Merge {
        /// Optional pattern to filter which dependencies to merge
        pattern: Option<String>,

        /// Perform the merge but do not create a commit
        #[arg(long)]
        no_commit: bool,

        /// Create a single non-merge commit instead of a merge commit
        #[arg(long)]
        squash: bool,

        /// Custom merge commit message
        #[arg(short, long)]
        message: Option<String>,
    },
}
