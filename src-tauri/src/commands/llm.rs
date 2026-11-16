use tauri::{AppHandle, State};

use crate::error::AppResult;
use crate::providers::groq::{self, ChatMessage};
use crate::state::AppState;

pub async fn reply(
    app: &AppHandle,
    state: &State<'_, AppState>,
    messages: Vec<ChatMessage>,
) -> AppResult<String> {
    groq::reply(app, state.inner(), messages).await
}
