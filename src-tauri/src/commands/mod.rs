mod llm;
mod stt;
mod tools;
mod tts;

use serde_json::Value;
use tauri::{AppHandle, State};

use crate::error::AppResult;
use crate::providers::groq::ChatMessage;
use crate::state::AppState;

#[tauri::command]
pub async fn stt_start(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    stt::start(app, state).await
}

#[tauri::command]
pub async fn stt_stop(state: State<'_, AppState>) -> AppResult<String> {
    stt::stop(state).await
}

#[tauri::command]
pub async fn tts_speak(app: AppHandle, state: State<'_, AppState>, text: String) -> AppResult<()> {
    tts::speak(app, state, text).await
}

#[tauri::command]
pub async fn tts_stop(state: State<'_, AppState>) -> AppResult<()> {
    tts::stop(state).await
}

#[tauri::command]
pub async fn llm_reply(
    app: AppHandle,
    state: State<'_, AppState>,
    messages: Vec<ChatMessage>,
) -> AppResult<String> {
    llm::reply(&app, &state, messages).await
}

#[tauri::command]
pub async fn tool_confirm_response(
    state: State<'_, AppState>,
    id: String,
    approved: bool,
) -> AppResult<()> {
    tools::confirm_response(state, id, approved).await
}

#[tauri::command]
pub async fn tool_execute(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    args: Value,
) -> AppResult<String> {
    tools::execute(app, state, name, args).await
}
