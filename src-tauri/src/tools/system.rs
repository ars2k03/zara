use std::fs;
use std::path::Path;

use serde_json::Value;
use tauri::AppHandle;

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::tools;
use crate::tools::shell;

pub async fn execute_get_system_info(_args: Value) -> AppResult<String> {
    let mut report = Vec::new();

    if let Ok(hostname) = fs::read_to_string("/etc/hostname") {
        report.push(format!("Hostname: {}", hostname.trim()));
    }

    if let Ok(uptime_raw) = fs::read_to_string("/proc/uptime")
        && let Some(secs_str) = uptime_raw.split_whitespace().next()
        && let Ok(secs) = secs_str.parse::<f64>()
    {
        let total = secs as u64;
        let hours = total / 3600;
        let mins = (total % 3600) / 60;
        report.push(format!("Uptime: {hours}h {mins}m"));
    }

    if let Ok(loadavg) = fs::read_to_string("/proc/loadavg") {
        let loads: Vec<&str> = loadavg.split_whitespace().take(3).collect();
        report.push(format!("CPU Load (1m 5m 15m): {}", loads.join(" ")));
    }

    if let Some(mem) = read_memory_summary() {
        report.push(mem);
    }

    if let Ok(df) = shell::run_program("df", &["-h".into(), "/".into()], None, 5).await
        && let Some(line) = df.stdout.lines().nth(1)
    {
        report.push(format!("Disk (/): {line}"));
    }

    if let Some(battery) = read_battery_summary() {
        report.push(battery);
    }

    if let Ok(ip) = shell::run_program("hostname", &["-I".into()], None, 5).await {
        let trimmed = ip.stdout.trim();
        if !trimmed.is_empty() {
            report.push(format!("IP: {trimmed}"));
        }
    }

    if let Ok(vol) = query_volume().await {
        report.push(format!("Volume: {vol}"));
    }

    Ok(report.join("\n"))
}

fn read_memory_summary() -> Option<String> {
    let raw = fs::read_to_string("/proc/meminfo").ok()?;
    let mut total_kb = 0u64;
    let mut avail_kb = 0u64;
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            total_kb = rest.split_whitespace().next()?.parse().ok()?;
        } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
            avail_kb = rest.split_whitespace().next()?.parse().ok()?;
        }
    }
    if total_kb == 0 {
        return None;
    }
    let used_gb = (total_kb.saturating_sub(avail_kb)) as f64 / (1024.0 * 1024.0);
    let total_gb = total_kb as f64 / (1024.0 * 1024.0);
    Some(format!("RAM: {used_gb:.1} GiB / {total_gb:.1} GiB used"))
}

fn read_battery_summary() -> Option<String> {
    let base = Path::new("/sys/class/power_supply");
    let entries = fs::read_dir(base).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("BAT") {
            continue;
        }
        let dir = entry.path();
        let cap = fs::read_to_string(dir.join("capacity")).ok()?;
        let status = fs::read_to_string(dir.join("status")).unwrap_or_default();
        return Some(format!("Battery ({name}): {}% ({})", cap.trim(), status.trim()));
    }
    None
}

async fn query_volume() -> AppResult<String> {
    if shell::command_exists("pactl") {
        let vol = shell::run_program(
            "pactl",
            &["get-sink-volume".into(), "@DEFAULT_SINK@".into()],
            None,
            5,
        )
        .await?;
        let mute = shell::run_program(
            "pactl",
            &["get-sink-mute".into(), "@DEFAULT_SINK@".into()],
            None,
            5,
        )
        .await?;
        return Ok(format!("{} ({})", vol.stdout.trim(), mute.stdout.trim()));
    }
    if shell::command_exists("wpctl") {
        let out = shell::run_program(
            "wpctl",
            &["get-volume".into(), "@DEFAULT_AUDIO_SINK@".into()],
            None,
            5,
        )
        .await?;
        return Ok(out.stdout.trim().to_string());
    }
    Err(AppError::from("Neither pactl nor wpctl is installed."))
}

pub async fn execute_set_volume(args: Value) -> AppResult<String> {
    let action = tools::req_str(&args, "action")?.to_lowercase();
    let value = args.get("value").and_then(|v| v.as_u64()).unwrap_or(10).min(150);

    if action == "get" {
        return query_volume().await;
    }

    if shell::command_exists("pactl") {
        let cmd_args: Vec<String> = match action.as_str() {
            "set" => vec!["set-sink-volume".into(), "@DEFAULT_SINK@".into(), format!("{value}%")],
            "up" => vec!["set-sink-volume".into(), "@DEFAULT_SINK@".into(), format!("+{value}%")],
            "down" => vec!["set-sink-volume".into(), "@DEFAULT_SINK@".into(), format!("-{value}%")],
            "mute" => vec!["set-sink-mute".into(), "@DEFAULT_SINK@".into(), "1".into()],
            "unmute" => vec!["set-sink-mute".into(), "@DEFAULT_SINK@".into(), "0".into()],
            "toggle_mute" => vec!["set-sink-mute".into(), "@DEFAULT_SINK@".into(), "toggle".into()],
            other => return Err(AppError::Message(format!("Unsupported volume action: {other}"))),
        };
        let out = shell::run_program("pactl", &cmd_args, None, 5).await?;
        if out.exit_code != 0 {
            return Err(AppError::Message(out.combined()));
        }
        return query_volume().await.or_else(|_| Ok(format!("Volume updated ({action}).")));
    }

    if shell::command_exists("amixer") {
        let arg = match action.as_str() {
            "set" => format!("{value}%"),
            "up" => format!("{value}%+"),
            "down" => format!("{value}%-"),
            "mute" => "mute".to_string(),
            "unmute" => "unmute".to_string(),
            "toggle_mute" => "toggle".to_string(),
            other => return Err(AppError::Message(format!("Unsupported volume action: {other}"))),
        };
        let out = shell::run_program("amixer", &["sset".into(), "Master".into(), arg], None, 5).await?;
        return Ok(out.combined());
    }

    Err(AppError::from("No supported audio control tool (pactl/amixer) found."))
}

pub async fn execute_set_brightness(args: Value) -> AppResult<String> {
    let action = tools::req_str(&args, "action")?.to_lowercase();
    let value = args.get("value").and_then(|v| v.as_u64()).unwrap_or(10).min(100);

    if shell::command_exists("brightnessctl") {
        let cmd_args: Vec<String> = match action.as_str() {
            "get" => vec!["-m".into()],
            "set" => vec!["set".into(), format!("{value}%")],
            "up" => vec!["set".into(), format!("+{value}%")],
            "down" => vec!["set".into(), format!("{value}%-")],
            other => return Err(AppError::Message(format!("Unsupported brightness action: {other}"))),
        };
        let out = shell::run_program("brightnessctl", &cmd_args, None, 5).await?;
        return Ok(out.combined());
    }

    Err(AppError::from("brightnessctl is not installed on this system."))
}

pub async fn execute_send_notification(args: Value) -> AppResult<String> {
    let title = tools::req_str(&args, "title")?;
    let body = tools::opt_str(&args, "body").unwrap_or_default();
    let urgency = tools::opt_str(&args, "urgency").unwrap_or_else(|| "normal".to_string());

    if !shell::command_exists("notify-send") {
        return Err(AppError::from("notify-send is not installed."));
    }

    let mut cmd_args = vec!["-u".to_string(), urgency, title.clone()];
    if !body.is_empty() {
        cmd_args.push(body);
    }
    let out = shell::run_program("notify-send", &cmd_args, None, 5).await?;
    if out.exit_code == 0 {
        Ok(format!("Sent notification: {title}"))
    } else {
        Err(AppError::Message(out.combined()))
    }
}

pub async fn execute_power_action(app: &AppHandle, state: &AppState, args: Value) -> AppResult<String> {
    let action = tools::req_str(&args, "action")?.to_lowercase();
    match action.as_str() {
        "lock" => {
            if shell::command_exists("loginctl") {
                let out = shell::run_program("loginctl", &["lock-session".into()], None, 5).await?;
                if out.exit_code == 0 {
                    return Ok("Screen locked.".to_string());
                }
            }
            if shell::command_exists("xdg-screensaver") {
                let out = shell::run_program("xdg-screensaver", &["lock".into()], None, 5).await?;
                return Ok(out.combined());
            }
            Err(AppError::from("No screen lock command available."))
        }
        "sleep" | "suspend" => {
            tools::confirm_action(
                app,
                state,
                "স্লিপ মোড কনফার্মেশন".to_string(),
                "আপনি কি সিস্টেমকে স্লিপ মোডে নিতে চান?".to_string(),
            )
            .await?;
            let out = shell::run_program("systemctl", &["suspend".into()], None, 10).await?;
            Ok(out.combined())
        }
        "reboot" | "restart" => {
            tools::confirm_action(
                app,
                state,
                "রিস্টার্ট কনফার্মেশন".to_string(),
                "আপনি কি পিসি রিস্টার্ট করতে চান?".to_string(),
            )
            .await?;
            let out = shell::run_program("systemctl", &["reboot".into()], None, 10).await?;
            Ok(out.combined())
        }
        "shutdown" | "poweroff" => {
            tools::confirm_action(
                app,
                state,
                "শাটডাউন কনফার্মেশন".to_string(),
                "আপনি কি পিসি শাটডাউন করতে চান?".to_string(),
            )
            .await?;
            let out = shell::run_program("systemctl", &["poweroff".into()], None, 10).await?;
            Ok(out.combined())
        }
        other => Err(AppError::Message(format!("Unsupported power action: {other}"))),
    }
}
