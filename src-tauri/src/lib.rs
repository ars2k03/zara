mod audio;
mod commands;
mod config;
mod error;
mod gemini;
mod providers;
mod state;
mod tools;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    config::load();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            config::env_status,
            commands::stt_start,
            commands::stt_stop,
            commands::tts_speak,
            commands::tts_stop,
            commands::llm_reply,
            commands::tool_confirm_response,
            commands::tool_execute,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
