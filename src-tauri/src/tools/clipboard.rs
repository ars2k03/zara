use std::env;

use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::tools;
use crate::tools::shell;

fn is_wayland() -> bool {
    env::var("WAYLAND_DISPLAY").is_ok_and(|value| !value.trim().is_empty())
}

pub async fn execute_clipboard_read(_args: Value) -> AppResult<String> {
    if is_wayland() && shell::command_exists("wl-paste") {
        let out = shell::run_program("wl-paste", &["--no-newline".into()], None, 5).await?;
        if out.exit_code == 0 {
            return Ok(out.stdout);
        }
    }
    if shell::command_exists("xclip") {
        let out = shell::run_program(
            "xclip",
            &["-o".into(), "-selection".into(), "clipboard".into()],
            None,
            5,
        )
        .await?;
        if out.exit_code == 0 {
            return Ok(out.stdout);
        }
        return Err(AppError::Message(out.combined()));
    }
    if shell::command_exists("xsel") {
        let out = shell::run_program("xsel", &["-ob".into()], None, 5).await?;
        if out.exit_code == 0 {
            return Ok(out.stdout);
        }
        return Err(AppError::Message(out.combined()));
    }
    Err(AppError::from("No clipboard utility (wl-paste/xclip/xsel) is installed."))
}

pub async fn execute_clipboard_write(args: Value) -> AppResult<String> {
    let text = tools::req_str(&args, "text")?;

    if is_wayland() && shell::command_exists("wl-copy") {
        let out = shell::run_program_with_stdin("wl-copy", &[], &text, 5).await?;
        if out.exit_code == 0 {
            return Ok(format!("Copied {} bytes to clipboard.", text.len()));
        }
    }
    if shell::command_exists("xclip") {
        let out = shell::run_program_with_stdin(
            "xclip",
            &["-selection".into(), "clipboard".into()],
            &text,
            5,
        )
        .await?;
        if out.exit_code == 0 {
            return Ok(format!("Copied {} bytes to clipboard.", text.len()));
        }
        return Err(AppError::Message(out.combined()));
    }
    if shell::command_exists("xsel") {
        let out = shell::run_program_with_stdin("xsel", &["-ib".into()], &text, 5).await?;
        if out.exit_code == 0 {
            return Ok(format!("Copied {} bytes to clipboard.", text.len()));
        }
        return Err(AppError::Message(out.combined()));
    }
    Err(AppError::from("No clipboard utility (wl-copy/xclip/xsel) is installed."))
}
