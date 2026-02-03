//! Toolchain helpers for executing external commands safely.

use std::process::Command;

use camino::Utf8Path;

/// Creates a command that is rooted to a specific working directory.
#[allow(clippy::disallowed_methods)]
pub fn command(program: &str, working_dir: &Utf8Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.current_dir(working_dir.as_std_path());
    cmd
}
