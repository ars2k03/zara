use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::tools;
use crate::tools::shell;

fn ensure_xdotool() -> AppResult<()> {
    if shell::command_exists("xdotool") {
        Ok(())
    } else {
        Err(AppError::from("xdotool is not installed."))
    }
}

pub async fn execute_type_text(args: Value) -> AppResult<String> {
    let text = tools::req_str(&args, "text")?;
    ensure_xdotool()?;
    let cmd_args = vec![
        "type".to_string(),
        "--clearmodifiers".to_string(),
        "--delay".to_string(),
        "12".to_string(),
        "--".to_string(),
        text.clone(),
    ];
    let out = shell::run_program("xdotool", &cmd_args, None, 15).await?;
    if out.exit_code == 0 {
        Ok(format!("Typed {} characters.", text.chars().count()))
    } else {
        Err(AppError::Message(out.combined()))
    }
}

pub async fn execute_press_key(args: Value) -> AppResult<String> {
    let keys = tools::req_str(&args, "keys")?;
    ensure_xdotool()?;
    let cmd_args = vec![
        "key".to_string(),
        "--clearmodifiers".to_string(),
        keys.clone(),
    ];
    let out = shell::run_program("xdotool", &cmd_args, None, 5).await?;
    if out.exit_code == 0 {
        Ok(format!("Pressed key combination: {keys}"))
    } else {
        Err(AppError::Message(out.combined()))
    }
}

pub async fn execute_mouse_action(args: Value) -> AppResult<String> {
    let action = tools::req_str(&args, "action")?.to_lowercase();
    let x = args.get("x").and_then(|v| v.as_i64());
    let y = args.get("y").and_then(|v| v.as_i64());
    let amount = args.get("amount").and_then(|v| v.as_u64()).unwrap_or(3).min(20);
    ensure_xdotool()?;

    if let (Some(px), Some(py)) = (x, y) {
        let out = shell::run_program(
            "xdotool",
            &["mousemove".into(), px.to_string(), py.to_string()],
            None,
            5,
        )
        .await?;
        if out.exit_code != 0 {
            return Err(AppError::Message(out.combined()));
        }
    }

    let cmd_args: Vec<String> = match action.as_str() {
        "move" => return Ok(format!("Moved mouse to ({}, {}).", x.unwrap_or(0), y.unwrap_or(0))),
        "click" | "left_click" => vec!["click".into(), "1".into()],
        "right_click" => vec!["click".into(), "3".into()],
        "middle_click" => vec!["click".into(), "2".into()],
        "double_click" => vec!["click".into(), "--repeat".into(), "2".into(), "1".into()],
        "scroll_up" => vec!["click".into(), "--repeat".into(), amount.to_string(), "4".into()],
        "scroll_down" => vec!["click".into(), "--repeat".into(), amount.to_string(), "5".into()],
        other => return Err(AppError::Message(format!("Unsupported mouse action: {other}"))),
    };

    let out = shell::run_program("xdotool", &cmd_args, None, 5).await?;
    if out.exit_code == 0 {
        Ok(format!("Performed mouse action: {action}"))
    } else {
        Err(AppError::Message(out.combined()))
    }
}
