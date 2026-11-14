use std::env;

use crate::error::{AppError, AppResult};

const API_KEYS: &[&str] = &["GROQ_API_KEY", "GOOGLE_API_KEY"];

pub fn load() {
    dotenvy::dotenv().ok();
}

pub fn require(name: &str) -> AppResult<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::Message(format!("{name} is not set.")))
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
            set: env::var(name).is_ok_and(|value| !value.trim().is_empty()),
        })
        .collect();
    let all_set = keys.iter().all(|key| key.set);
    EnvStatus { all_set, keys }
}
