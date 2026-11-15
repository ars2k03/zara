mod llm;
mod stt;
mod tts;

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
pub async fn llm_reply(messages: Vec<ChatMessage>) -> AppResult<String> {
    llm::reply(messages).await
}
