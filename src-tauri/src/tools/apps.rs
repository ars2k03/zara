use serde_json::Value;
use tauri::AppHandle;

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::tools;
use crate::tools::filesystem;
use crate::tools::shell;

fn resolve_app_binary(name: &str) -> String {
    let normalized = name.trim().to_lowercase();
    let candidates: &[&str] = match normalized.as_str() {
        "mozilla" | "firefox" | "mozilla firefox" => &["firefox"],
        "chrome" | "google chrome" | "google-chrome" => &["google-chrome", "chromium"],
        "chromium" => &["chromium", "chromium-browser"],
        "vscode" | "vs code" | "code" | "visual studio code" => &["code"],
        "terminal" | "bash" | "shell" => &[
            "gnome-terminal",
            "konsole",
            "xfce4-terminal",
            "kitty",
            "alacritty",
            "x-terminal-emulator",
        ],
        "files" | "file manager" | "explorer" | "nautilus" | "dolphin" | "thunar" | "nemo"
        | "pcmanfm" => &[
            "thunar",
            "nautilus",
            "dolphin",
            "nemo",
            "pcmanfm",
            "xdg-open",
        ],
        "calculator" | "calc" => &["gnome-calculator", "kcalc", "galculator"],
        "spotify" => &["spotify"],
        "vlc" => &["vlc"],
        "discord" => &["discord"],
        "obsidian" => &["obsidian"],
        _ => return name.trim().to_string(),
    };

    candidates
        .iter()
        .copied()
        .find(|bin| shell::command_exists(bin))
        .unwrap_or(candidates[0])
        .to_string()
}

pub async fn execute_launch_app(args: Value) -> AppResult<String> {
    let name = tools::req_str(&args, "name")?;
    let binary = resolve_app_binary(&name);
    let extra_args: Vec<String> = args
        .get("args")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_str())
                .map(expand_arg_tilde)
                .collect()
        })
        .unwrap_or_default();

    let pid = shell::spawn_detached(&binary, &extra_args)?;
    Ok(format!("Launched {binary} (PID {pid})."))
}

fn expand_arg_tilde(arg: &str) -> String {
    if (arg == "~" || arg.starts_with("~/"))
        && let Ok(resolved) = filesystem::resolve_path(arg)
    {
        return resolved.to_string_lossy().into_owned();
    }
    arg.to_string()
}

pub async fn execute_list_processes(args: Value) -> AppResult<String> {
    let filter = tools::opt_str(&args, "filter").map(|s| s.to_lowercase());
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20).min(50) as usize;

    let ps_args = vec![
        "-eo".to_string(),
        "pid,comm,%cpu,%mem".to_string(),
        "--sort=-%mem".to_string(),
    ];
    let output = shell::run_program("ps", &ps_args, None, 10).await?;

    let mut lines = Vec::new();
    for (idx, line) in output.stdout.lines().enumerate() {
        if idx == 0 {
            lines.push(line.to_string());
            continue;
        }
        if let Some(ref f) = filter
            && !line.to_lowercase().contains(f)
        {
            continue;
        }
        lines.push(line.to_string());
        if lines.len() > limit {
            break;
        }
    }

    if lines.len() <= 1 {
        Ok("No matching processes found.".to_string())
    } else {
        Ok(lines.join("\n"))
    }
}

pub async fn execute_kill_process(app: &AppHandle, state: &AppState, args: Value) -> AppResult<String> {
    let pid = args.get("pid").and_then(|v| v.as_u64());
    let name = tools::opt_str(&args, "name");
    let force = args.get("force").and_then(|v| v.as_bool()).unwrap_or(false);

    if pid.is_none() && name.is_none() {
        return Err(AppError::from("Provide either `pid` or `name`."));
    }

    let target_desc = match (pid, &name) {
        (Some(p), _) => format!("PID {p}"),
        (None, Some(n)) => format!("process \"{n}\""),
        _ => unreachable!(),
    };

    if force || name.is_some() {
        tools::confirm_action(
            app,
            state,
            "প্রসেস বন্ধ কনফার্মেশন".to_string(),
            format!("আপনি কি {target_desc} বন্ধ করতে চান?"),
        )
        .await?;
    }

    let output = if let Some(p) = pid {
        let mut kill_args = Vec::new();
        if force {
            kill_args.push("-9".to_string());
        }
        kill_args.push(p.to_string());
        shell::run_program("kill", &kill_args, None, 10).await?
    } else if let Some(n) = name {
        let mut pkill_args = Vec::new();
        if force {
            pkill_args.push("-9".to_string());
        }
        pkill_args.push("-f".to_string());
        pkill_args.push(n);
        shell::run_program("pkill", &pkill_args, None, 10).await?
    } else {
        unreachable!()
    };

    if output.exit_code == 0 {
        Ok(format!("Terminated {target_desc}."))
    } else {
        Err(AppError::Message(format!(
            "Failed to terminate {target_desc}: {}",
            output.combined()
        )))
    }
}
