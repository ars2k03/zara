mod config;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    config::load();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![config::env_status])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
