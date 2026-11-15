use tauri::{AppHandle, State};

use crate::config;
use crate::error::AppResult;
use crate::gemini::tts;
use crate::state::AppState;

pub async fn speak(app: AppHandle, state: State<'_, AppState>, text: String) -> AppResult<()> {
    let api_key = config::require("GOOGLE_API_KEY")?;
    tts::speak(app, state, &api_key, &text).await
}

pub async fn stop(state: State<'_, AppState>) -> AppResult<()> {
    tts::stop(state).await
}
