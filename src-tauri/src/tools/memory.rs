use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::error::{AppError, AppResult};
use crate::tools;
use crate::tools::filesystem;

const MAX_PROMPT_ENTRIES: usize = 50;

static MEMORY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn lock() -> &'static Mutex<()> {
    MEMORY_LOCK.get_or_init(|| Mutex::new(()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: u64,
    pub category: String,
    pub content: String,
    pub created_at: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct MemoryStore {
    next_id: u64,
    entries: Vec<MemoryEntry>,
}

fn memory_file_path() -> AppResult<PathBuf> {
    if let Ok(xdg) = env::var("XDG_DATA_HOME")
        && !xdg.trim().is_empty()
    {
        return Ok(PathBuf::from(xdg).join("zara").join("memory.json"));
    }
    Ok(filesystem::home_dir()?
        .join(".local")
        .join("share")
        .join("zara")
        .join("memory.json"))
}

fn load_store_from(path: &Path) -> AppResult<MemoryStore> {
    if !path.exists() {
        return Ok(MemoryStore::default());
    }
    let raw = fs::read_to_string(path)?;
    if raw.trim().is_empty() {
        return Ok(MemoryStore::default());
    }
    serde_json::from_str(&raw).map_err(AppError::from)
}

fn save_store_to(path: &Path, store: &MemoryStore) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(store)?;
    fs::write(path, json)?;
    Ok(())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub async fn format_for_system_prompt() -> String {
    let _guard = lock().lock().await;
    let Ok(path) = memory_file_path() else {
        return String::new();
    };
    let Ok(store) = load_store_from(&path) else {
        return String::new();
    };
    format_entries(&store.entries)
}

pub(crate) fn format_entries(entries: &[MemoryEntry]) -> String {
    if entries.is_empty() {
        return String::new();
    }
    let start = entries.len().saturating_sub(MAX_PROMPT_ENTRIES);
    entries[start..]
        .iter()
        .map(|entry| format!("- [id:{} | {}] {}", entry.id, entry.category, entry.content))
        .collect::<Vec<_>>()
        .join("\n")
}

pub async fn execute_save_memory(args: Value) -> AppResult<String> {
    let content = tools::req_str(&args, "content")?.trim().to_string();
    if content.is_empty() {
        return Err(AppError::from("Memory content cannot be empty."));
    }
    let category = tools::opt_str(&args, "category").unwrap_or_else(|| "fact".to_string());
    let path = memory_file_path()?;

    let _guard = lock().lock().await;
    save_memory_at(&path, content, category)
}

pub(crate) fn save_memory_at(
    path: &Path,
    content: String,
    category: String,
) -> AppResult<String> {
    let mut store = load_store_from(path)?;
    let normalized = content.to_lowercase();

    if let Some(existing) = store
        .entries
        .iter()
        .find(|entry| entry.content.to_lowercase() == normalized)
    {
        return Ok(format!(
            "Memory already saved as #{} ({}): {}",
            existing.id, existing.category, existing.content
        ));
    }

    store.next_id += 1;
    let id = store.next_id;
    store.entries.push(MemoryEntry {
        id,
        category: category.clone(),
        content: content.clone(),
        created_at: now_secs(),
    });
    save_store_to(path, &store)?;
    Ok(format!("Saved memory #{id} ({category}): {content}"))
}

pub async fn execute_search_memory(args: Value) -> AppResult<String> {
    let query = tools::opt_str(&args, "query");
    let path = memory_file_path()?;

    let _guard = lock().lock().await;
    search_memory_at(&path, query.as_deref())
}

pub(crate) fn search_memory_at(path: &Path, query: Option<&str>) -> AppResult<String> {
    let store = load_store_from(path)?;
    if store.entries.is_empty() {
        return Ok("No saved memories yet.".to_string());
    }

    let matches: Vec<&MemoryEntry> = match query {
        Some(q) => {
            let needle = q.to_lowercase();
            store
                .entries
                .iter()
                .filter(|entry| {
                    entry.content.to_lowercase().contains(&needle)
                        || entry.category.to_lowercase().contains(&needle)
                })
                .collect()
        }
        None => store.entries.iter().collect(),
    };

    if matches.is_empty() {
        return Ok("No matching memories found.".to_string());
    }

    Ok(matches
        .into_iter()
        .map(|entry| format!("#{} [{}]: {}", entry.id, entry.category, entry.content))
        .collect::<Vec<_>>()
        .join("\n"))
}

pub async fn execute_forget_memory(args: Value) -> AppResult<String> {
    let id = args.get("id").and_then(|v| v.as_u64());
    let query = tools::opt_str(&args, "query");
    if id.is_none() && query.is_none() {
        return Err(AppError::from("Provide either `id` or `query` to forget memory."));
    }
    let path = memory_file_path()?;

    let _guard = lock().lock().await;
    forget_memory_at(&path, id, query.as_deref())
}

pub(crate) fn forget_memory_at(
    path: &Path,
    id: Option<u64>,
    query: Option<&str>,
) -> AppResult<String> {
    let mut store = load_store_from(path)?;
    let before = store.entries.len();

    if let Some(target_id) = id {
        store.entries.retain(|entry| entry.id != target_id);
    } else if let Some(q) = query {
        let needle = q.to_lowercase();
        store
            .entries
            .retain(|entry| !entry.content.to_lowercase().contains(&needle));
    }

    let removed = before.saturating_sub(store.entries.len());
    if removed == 0 {
        return Ok("No matching memory found to remove.".to_string());
    }

    save_store_to(path, &store)?;
    Ok(format!("Removed {removed} memory item(s)."))
}
