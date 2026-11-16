use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::oneshot;

use crate::error::{AppError, AppResult};
use crate::state::{ActiveTimer, AppState};
use crate::tools;
use crate::tools::shell;

#[derive(Clone, Serialize)]
pub struct TimerFiredEvent {
    pub id: String,
    pub label: String,
    pub seconds: u64,
}

pub async fn execute_set_timer(app: &AppHandle, state: &AppState, args: Value) -> AppResult<String> {
    let seconds = args
        .get("seconds")
        .and_then(|v| v.as_u64())
        .filter(|&s| s > 0)
        .ok_or_else(|| AppError::from("`seconds` must be a positive integer."))?;
    let label = tools::opt_str(&args, "label").unwrap_or_else(|| "Timer".to_string());

    let id_num = state.next_id.fetch_add(1, Ordering::Relaxed) + 1;
    let id = format!("timer-{id_num}");
    let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
    let ends_at = Instant::now() + Duration::from_secs(seconds);

    {
        let mut timers = state.active_timers.lock().await;
        timers.insert(
            id.clone(),
            ActiveTimer {
                label: label.clone(),
                ends_at,
                cancel: cancel_tx,
            },
        );
    }

    let app_handle = app.clone();
    let timer_id = id.clone();
    let timer_label = label.clone();
    tokio::spawn(async move {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(seconds)) => {
                let state_ref = app_handle.state::<AppState>();
                let mut timers = state_ref.active_timers.lock().await;
                timers.remove(&timer_id);
                drop(timers);

                if shell::command_exists("notify-send") {
                    let _ = shell::run_program(
                        "notify-send",
                        &["Zara Timer".into(), format!("সময় শেষ: {timer_label}")],
                        None,
                        5,
                    )
                    .await;
                }

                let _ = app_handle.emit(
                    "timer://fired",
                    TimerFiredEvent {
                        id: timer_id,
                        label: timer_label,
                        seconds,
                    },
                );
            }
            _ = cancel_rx => {}
        }
    });

    Ok(format!("Set timer `{id}` ({label}) for {seconds} seconds."))
}

pub async fn execute_list_timers(state: &AppState) -> AppResult<String> {
    let now = Instant::now();
    let timers = state.active_timers.lock().await;
    if timers.is_empty() {
        return Ok("No active timers.".to_string());
    }

    let lines: Vec<String> = timers
        .iter()
        .map(|(id, timer)| {
            let remaining = timer.ends_at.saturating_duration_since(now).as_secs();
            format!("{id}: {} ({remaining}s remaining)", timer.label)
        })
        .collect();
    Ok(lines.join("\n"))
}

pub async fn execute_cancel_timer(state: &AppState, args: Value) -> AppResult<String> {
    let id = tools::req_str(&args, "id")?;
    let mut timers = state.active_timers.lock().await;
    let removed = timers
        .remove(&id)
        .ok_or_else(|| AppError::Message(format!("Timer `{id}` not found.")))?;
    let _ = removed.cancel.send(());
    Ok(format!("Cancelled timer `{id}` ({}).", removed.label))
}
