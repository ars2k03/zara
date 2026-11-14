use std::env;

const API_KEYS: &[&str] = &["GROQ_API_KEY", "GOOGLE_API_KEY"];

pub fn load() {
    dotenvy::dotenv().ok();
}

#[derive(serde::Serialize)]
pub struct KeyStatus {
    pub name: String,
    pub set: bool,
}

#[derive(serde::Serialize)]
pub struct EnvStatus {
    pub all_set: bool,
    pub keys: Vec<KeyStatus>,
}

#[tauri::command]
pub fn env_status() -> EnvStatus {
    let keys: Vec<KeyStatus> = API_KEYS
        .iter()
        .map(|&name| KeyStatus {
            name: name.to_string(),
            set: env::var(name).is_ok_and(|v| !v.trim().is_empty()),
        })
        .collect();
    let all_set = keys.iter().all(|k| k.set);
    EnvStatus { all_set, keys }
}
