//! External editor integration for opening files from the TUI.

use std::env;
use std::path::Path;

use camino::{Utf8Path, Utf8PathBuf};
use ch_core::Config;

use crate::error::TuiError;
use crate::toolchain;
use crate::tui::Tui;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditorKind {
    Cursor,
    VsCode,
    Nvim,
    Vim,
    Nano,
    Other,
}

#[derive(Debug, Clone)]
struct EditorCommand {
    program: String,
    args: Vec<String>,
    kind: EditorKind,
}

impl EditorCommand {
    fn with_wait_flag(mut self) -> Self {
        if matches!(self.kind, EditorKind::Cursor | EditorKind::VsCode)
            && !self
                .args
                .iter()
                .any(|arg| arg == "--wait" || arg == "-w")
        {
            self.args.push("--wait".to_owned());
        }
        self
    }
}

fn parse_editor_command(command: &str) -> Option<EditorCommand> {
    let mut parts = command.split_whitespace();
    let program = parts.next()?.to_owned();
    let args = parts.map(str::to_owned).collect::<Vec<_>>();
    let kind = editor_kind_from_program(&program);

    Some(EditorCommand {
        program,
        args,
        kind,
    })
}

fn editor_kind_from_program(program: &str) -> EditorKind {
    let file_name = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program)
        .to_lowercase();

    match file_name.as_str() {
        "cursor" | "cursor.exe" => EditorKind::Cursor,
        "code" | "code-insiders" | "code.exe" => EditorKind::VsCode,
        "nvim" | "nvim.exe" => EditorKind::Nvim,
        "vim" | "vim.exe" => EditorKind::Vim,
        "nano" | "nano.exe" => EditorKind::Nano,
        _ => EditorKind::Other,
    }
}

fn resolve_editor(config: &Config) -> Result<EditorCommand, TuiError> {
    let mut candidates = Vec::new();

    if let Some(editor) = config.editor.editor.as_ref() {
        candidates.push(editor.clone());
    } else if let Ok(editor) = env::var("VISUAL") {
        candidates.push(editor);
    } else if let Ok(editor) = env::var("EDITOR") {
        candidates.push(editor);
    } else {
        candidates.extend([
            "cursor",
            "code",
            "nvim",
            "vim",
            "nano",
        ]
        .into_iter()
        .map(str::to_owned));
    }

    for candidate in candidates {
        if let Some(cmd) = parse_editor_command(&candidate) {
            return Ok(cmd.with_wait_flag());
        }
    }

    Err(TuiError::config(
        "No editor configured. Set --editor, $VISUAL, or $EDITOR.",
    ))
}

fn resolve_absolute_path(path: &Utf8Path, root: &Utf8Path) -> Utf8PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

/// Runs the external editor, suspending the TUI while it is active.
pub fn run_editor(
    path: &Utf8Path,
    root: &Utf8Path,
    config: &Config,
    tui: &mut Tui,
) -> Result<(), TuiError> {
    let editor = resolve_editor(config)?;
    let absolute_path = resolve_absolute_path(path, root);

    tui.exit()?;

    let editor_result = (|| {
        let mut command = toolchain::command(&editor.program, root);
        command.args(&editor.args).arg(absolute_path.as_str());

        let status = command.status()?;
        if status.success() {
            Ok(())
        } else {
            Err(TuiError::config(format!(
                "Editor exited with status: {status}"
            )))
        }
    })();

    tui.enter()?;

    editor_result
}
