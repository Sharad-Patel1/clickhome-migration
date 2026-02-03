//! Build automation tasks for the ch-migration workspace.
//!
//! Run with: `cargo xt <command>`
//!
//! # Available Commands
//!
//! - `check`: Run all checks (fmt, clippy, test)
//! - `fmt`: Format code with rustfmt
//! - `lint`: Run clippy with all targets
//! - `test`: Run all tests
//! - `build`: Build release binary
//! - `clean`: Clean build artifacts

// xtask is a build tool - printing to stderr is expected
#![allow(clippy::print_stderr)]
// Will return errors when implemented
#![allow(clippy::unnecessary_wraps)]

use anyhow::Result;
use clap::{Parser, Subcommand};

/// Build automation for ch-migration
#[derive(Parser)]
#[command(name = "xtask")]
#[command(about = "Build automation tasks for ch-migration")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run all checks (fmt --check, clippy, test)
    Check,
    /// Format code with rustfmt
    Fmt {
        /// Check formatting without modifying files
        #[arg(long)]
        check: bool,
    },
    /// Run clippy lints
    Lint {
        /// Automatically fix lint warnings
        #[arg(long)]
        fix: bool,
    },
    /// Run all tests
    Test {
        /// Run tests with release optimizations
        #[arg(long)]
        release: bool,
    },
    /// Build release binary
    Build {
        /// Build in debug mode
        #[arg(long)]
        debug: bool,
    },
    /// Clean build artifacts
    Clean,
    /// Generate documentation
    Doc {
        /// Open in browser after building
        #[arg(long)]
        open: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Check => {
            // TODO: Implement check command
            eprintln!("xtask check: not yet implemented");
        }
        Commands::Fmt { check } => {
            // TODO: Implement fmt command
            eprintln!("xtask fmt (check={check}): not yet implemented");
        }
        Commands::Lint { fix } => {
            // TODO: Implement lint command
            eprintln!("xtask lint (fix={fix}): not yet implemented");
        }
        Commands::Test { release } => {
            // TODO: Implement test command
            eprintln!("xtask test (release={release}): not yet implemented");
        }
        Commands::Build { debug } => {
            // TODO: Implement build command
            eprintln!("xtask build (debug={debug}): not yet implemented");
        }
        Commands::Clean => {
            // TODO: Implement clean command
            eprintln!("xtask clean: not yet implemented");
        }
        Commands::Doc { open } => {
            // TODO: Implement doc command
            eprintln!("xtask doc (open={open}): not yet implemented");
        }
    }

    Ok(())
}
