use tauri::AppHandle;

use crate::config;
use crate::error::AppResult;
use crate::gemini::tts;

pub async fn speak(app: AppHandle, text: String) -> AppResult<()> {
    let api_key = config::require("GOOGLE_API_KEY")?;
    tts::speak(app, &api_key, &text).await
}
