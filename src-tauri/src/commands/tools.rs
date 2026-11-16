use serde_json::Value;
use tauri::{AppHandle, State};

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::tools;

pub async fn confirm_response(
    state: State<'_, AppState>,
    id: String,
    approved: bool,
) -> AppResult<()> {
    let mut pending = state.pending_confirmations.lock().await;
    let sender = pending
        .remove(&id)
        .ok_or_else(|| AppError::Message(format!("Confirmation `{id}` is no longer active.")))?;
    let _ = sender.send(approved);
    Ok(())
}

pub async fn execute(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    args: Value,
) -> AppResult<String> {
    tools::execute(&app, state.inner(), &name, args).await
}
