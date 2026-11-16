use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::tools;
use crate::tools::filesystem;
use crate::tools::shell;

pub async fn execute_take_screenshot(args: Value) -> AppResult<String> {
    let mode = tools::opt_str(&args, "mode").unwrap_or_else(|| "full".to_string());
    let custom_name = tools::opt_str(&args, "filename");

    let dir = filesystem::home_dir()
        .map(|home| home.join("Pictures").join("Zara"))
        .unwrap_or_else(|_| PathBuf::from("/tmp"));
    fs::create_dir_all(&dir)?;

    let filename = custom_name.unwrap_or_else(|| {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        format!("zara-screenshot-{ts}.png")
    });
    let target = dir.join(filename);
    let path_str = target.to_string_lossy().into_owned();

    let is_wayland = env::var("WAYLAND_DISPLAY").is_ok_and(|v| !v.trim().is_empty());
    if is_wayland && shell::command_exists("grim") {
        let out = shell::run_program("grim", std::slice::from_ref(&path_str), None, 10).await?;
        if out.exit_code == 0 {
            return Ok(format!("Saved screenshot to {path_str}"));
        }
    }

    if shell::command_exists("scrot") {
        let mut cmd_args = Vec::new();
        if mode == "window" {
            cmd_args.push("-u".into());
        }
        cmd_args.push(path_str.clone());
        let out = shell::run_program("scrot", &cmd_args, None, 10).await?;
        if out.exit_code == 0 {
            return Ok(format!("Saved screenshot to {path_str}"));
        }
    }

    if shell::command_exists("gnome-screenshot") {
        let mut cmd_args = vec!["-f".into(), path_str.clone()];
        if mode == "window" {
            cmd_args.push("-w".into());
        }
        let out = shell::run_program("gnome-screenshot", &cmd_args, None, 10).await?;
        if out.exit_code == 0 {
            return Ok(format!("Saved screenshot to {path_str}"));
        }
    }

    if shell::command_exists("import") {
        let target_win = if mode == "window" { "active" } else { "root" };
        let out = shell::run_program("import", &["-window".into(), target_win.into(), path_str.clone()], None, 10).await?;
        if out.exit_code == 0 {
            return Ok(format!("Saved screenshot to {path_str}"));
        }
    }

    Err(AppError::from(
        "No screenshot tool found (install scrot, grim, gnome-screenshot, or imagemagick).",
    ))
}
