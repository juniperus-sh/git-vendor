mod cli;

use clap::Parser;
use cli::{Cli, Commands};
use git_vendor::Vendor;
use git2 as git;
use std::process;

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Open the repository in current directory
    let repo = git::Repository::open(".")?;

    match cli.command {
        Commands::Track {
            pattern,
            url,
            branch,
            prefix,
        } => {
            repo.track_pattern(&pattern, &url, branch.as_deref(), prefix.as_deref())?;
            println!("Tracked pattern: {}", pattern);
            if let Some(ref p) = prefix {
                println!("  prefix: {}", p);
            }
            println!("  url: {}", url);
            if let Some(ref b) = branch {
                println!("  branch: {}", b);
            }
        }

        Commands::Untrack { pattern } => {
            repo.untrack_pattern(&pattern)?;
            println!("Untracked pattern: {}", pattern);
        }

        Commands::Status { pattern } => {
            repo.vendor_status(pattern.as_deref())?;
        }

        Commands::Fetch { pattern } => {
            repo.vendor_fetch(pattern.as_deref(), None)?;
        }

        Commands::Merge { pattern, .. } => {
            repo.vendor_merge(pattern.as_deref(), None)?;
        }
    }

    Ok(())
}
