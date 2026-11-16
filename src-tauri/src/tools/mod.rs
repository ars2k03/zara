pub mod apps;
pub mod browser;
pub mod clipboard;
pub mod filesystem;
pub mod input;
pub mod memory;
pub mod screen;
pub mod shell;
pub mod system;
pub mod timer;
pub mod web;

use std::sync::atomic::Ordering;
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

const CONFIRM_TIMEOUT_SECS: u64 = 60;

#[derive(Clone, Serialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub function: FunctionDefinition,
}

#[derive(Clone, Serialize)]
pub struct FunctionDefinition {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
}

#[derive(Clone, Serialize)]
struct ToolActivityEvent {
    name: String,
    ok: Option<bool>,
    summary: Option<String>,
}

#[derive(Clone, Serialize)]
struct ToolConfirmRequest {
    id: String,
    title: String,
    detail: String,
}

pub fn req_str(args: &Value, key: &str) -> AppResult<String> {
    args.get(key)
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
        .ok_or_else(|| AppError::Message(format!("Missing required string argument `{key}`.")))
}

pub fn opt_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub async fn confirm_action(
    app: &AppHandle,
    state: &AppState,
    title: String,
    detail: String,
) -> AppResult<()> {
    let id_num = state.next_id.fetch_add(1, Ordering::Relaxed) + 1;
    let id = format!("confirm-{id_num}");
    let (tx, rx) = oneshot::channel::<bool>();

    {
        let mut pending = state.pending_confirmations.lock().await;
        pending.insert(id.clone(), tx);
    }

    app.emit(
        "tool://confirm",
        ToolConfirmRequest {
            id: id.clone(),
            title,
            detail,
        },
    )
    .map_err(|err| AppError::Message(format!("Failed to emit confirmation prompt: {err}")))?;

    let approved = tokio::time::timeout(Duration::from_secs(CONFIRM_TIMEOUT_SECS), rx)
        .await
        .map_err(|_| AppError::from("Confirmation timed out."))?
        .map_err(|_| AppError::from("Confirmation channel closed."))?;

    let mut pending = state.pending_confirmations.lock().await;
    pending.remove(&id);

    if approved {
        Ok(())
    } else {
        Err(AppError::from("Action cancelled by user."))
    }
}

pub async fn execute(
    app: &AppHandle,
    state: &AppState,
    name: &str,
    args: Value,
) -> AppResult<String> {
    let _ = app.emit(
        "tool://start",
        ToolActivityEvent {
            name: name.to_string(),
            ok: None,
            summary: None,
        },
    );

    let result = dispatch(app, state, name, args).await;

    let (ok, summary) = match &result {
        Ok(output) => (true, first_line_preview(output)),
        Err(err) => (false, first_line_preview(&err.to_string())),
    };

    let _ = app.emit(
        "tool://finish",
        ToolActivityEvent {
            name: name.to_string(),
            ok: Some(ok),
            summary: Some(summary),
        },
    );

    result
}

fn first_line_preview(text: &str) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    if line.chars().count() <= 90 {
        line.to_string()
    } else {
        let preview: String = line.chars().take(87).collect();
        format!("{preview}...")
    }
}

async fn dispatch(
    app: &AppHandle,
    state: &AppState,
    name: &str,
    args: Value,
) -> AppResult<String> {
    match name {
        "run_shell" => shell::execute_run_shell(app, state, args).await,
        "read_file" => filesystem::execute_read_file(args).await,
        "write_file" => filesystem::execute_write_file(args).await,
        "edit_file" => filesystem::execute_edit_file(args).await,
        "list_dir" => filesystem::execute_list_dir(args).await,
        "search_files" => filesystem::execute_search_files(args).await,
        "manage_path" => filesystem::execute_manage_path(app, state, args).await,
        "open_url" => browser::execute_open_url(args).await,
        "browser_search" => browser::execute_browser_search(args).await,
        "play_youtube" => browser::execute_play_youtube(args).await,
        "launch_app" => apps::execute_launch_app(args).await,
        "list_processes" => apps::execute_list_processes(args).await,
        "kill_process" => apps::execute_kill_process(app, state, args).await,
        "get_system_info" => system::execute_get_system_info(args).await,
        "set_volume" => system::execute_set_volume(args).await,
        "set_brightness" => system::execute_set_brightness(args).await,
        "send_notification" => system::execute_send_notification(args).await,
        "power_action" => system::execute_power_action(app, state, args).await,
        "clipboard_read" => clipboard::execute_clipboard_read(args).await,
        "clipboard_write" => clipboard::execute_clipboard_write(args).await,
        "type_text" => input::execute_type_text(args).await,
        "press_key" => input::execute_press_key(args).await,
        "mouse_action" => input::execute_mouse_action(args).await,
        "take_screenshot" => screen::execute_take_screenshot(args).await,
        "web_fetch" => web::execute_web_fetch(args).await,
        "web_search" => web::execute_web_search(args).await,
        "set_timer" => timer::execute_set_timer(app, state, args).await,
        "list_timers" => timer::execute_list_timers(state).await,
        "cancel_timer" => timer::execute_cancel_timer(state, args).await,
        "save_memory" => memory::execute_save_memory(args).await,
        "search_memory" => memory::execute_search_memory(args).await,
        "forget_memory" => memory::execute_forget_memory(args).await,
        unknown => Err(AppError::Message(format!("Unknown tool: {unknown}"))),
    }
}

fn tool(name: &'static str, description: &'static str, parameters: Value) -> ToolDefinition {
    ToolDefinition {
        kind: "function",
        function: FunctionDefinition {
            name,
            description,
            parameters,
        },
    }
}

pub fn select_definitions(user_text: &str) -> Vec<ToolDefinition> {
    let lower = user_text.to_lowercase();
    let mut selected: Vec<&'static str> = Vec::new();

    let mut add = |names: &[&'static str]| {
        for &name in names {
            if !selected.contains(&name) {
                selected.push(name);
            }
        }
    };

    if has_any(&lower, &[
        "youtube", "ইউটিউব", "firefox", "mozilla", "chrome", "chromium", "brave",
        "browser", "ব্রাউজার", "google", "গুগল", "search", "সার্চ", "খোঁজ", "খোজ",
        "url", "link", "website", "ওয়েবসাইট", "ওয়েবসাইট", "song", "গান", "video",
        "ভিডিও", "play", "চালাও", "প্লে", "news", "খবর", "weather", "আবহাওয়া",
        "আবহাওয়া", "internet", "ইন্টারনেট", "web",
    ]) {
        add(&["open_url", "browser_search", "play_youtube", "web_search", "web_fetch", "launch_app"]);
    }

    if has_any(&lower, &[
        "file", "ফাইল", "folder", "ফোল্ডার", "directory", "ডিরেক্টরি", "download",
        "ডাউনলোড", "document", "ডকুমেন্ট", "desktop", "ডেস্কটপ", "picture", "পিকচার",
        "delete", "ডিলিট", "মুছ", "remove", "move", "সরাও", "copy", "কপি", "rename",
        "রিনেম", "create", "তৈরি", "write", "লিখ", "read", "পড়", "পড়", "edit",
        "এডিট", "find", "খুঁজ", "trash", "রিসাইকেল",
    ]) {
        add(&["read_file", "write_file", "edit_file", "list_dir", "search_files", "manage_path"]);
    }

    if has_any(&lower, &[
        "open", "ওপেন", "খোল", "খুল", "launch", "start", "চালু", "app", "অ্যাপ",
        "vscode", "vs code", "code", "terminal", "টার্মিনাল", "spotify", "vlc",
        "discord",
    ]) {
        add(&["launch_app", "manage_path", "open_url"]);
    }

    if has_any(&lower, &["process", "প্রসেস", "kill", "close", "বন্ধ"]) {
        add(&["list_processes", "kill_process"]);
    }

    if has_any(&lower, &[
        "system", "সিস্টেম", "cpu", "ram", "র‍্যাম", "disk", "ডিস্ক", "storage",
        "battery", "ব্যাটারি", "charge", "চার্জ", "wifi", "ওয়াইফাই", "ওআইফাই",
        "volume", "ভলিউম", "sound", "সাউন্ড", "আওয়াজ", "mute", "মিউট",
        "brightness", "ব্রাইটনেস", "আলো", "notify", "নোটিফিকেশন", "lock", "লক",
        "sleep", "স্লিপ", "restart", "রিস্টার্ট", "reboot", "রিবুট", "shutdown",
        "শাটডাউন",
    ]) {
        add(&["get_system_info", "set_volume", "set_brightness", "send_notification", "power_action"]);
    }

    if has_any(&lower, &["clipboard", "ক্লিপবোর্ড", "copied", "কপি করা", "paste", "পেস্ট"]) {
        add(&["clipboard_read", "clipboard_write"]);
    }

    if has_any(&lower, &[
        "type", "টাইপ", "press", "প্রেস", "shortcut", "শর্টকাট", "mouse", "মাউস",
        "click", "ক্লিক", "scroll", "স্ক্রল",
    ]) {
        add(&["type_text", "press_key", "mouse_action"]);
    }

    if has_any(&lower, &["screenshot", "স্ক্রিনশট", "screen", "স্ক্রিন", "capture"]) {
        add(&["take_screenshot"]);
    }

    if has_any(&lower, &[
        "timer", "টাইমার", "alarm", "অ্যালার্ম", "remind", "রিমাইন্ড",
        "মনে করিয়ে", "মনে করিয়ে", "minute", "মিনিট", "second", "সেকেন্ড",
        "hour", "ঘণ্টা", "ঘন্টা",
    ]) {
        add(&["set_timer", "list_timers", "cancel_timer"]);
    }

    if has_any(&lower, &[
        "remember", "মনে রাখ", "মনে আছে", "forget", "ভুলে যা", "পছন্দ",
        "আমার নাম", "my name", "memory", "মেমোরি",
    ]) {
        add(&["save_memory", "search_memory", "forget_memory"]);
    }

    if has_any(&lower, &[
        "shell", "bash", "command", "কমান্ড", "git", "docker", "npm", "cargo",
        "python", "script", "স্ক্রিপ্ট", "install", "ইনস্টল", "sudo", "run",
    ]) {
        add(&["run_shell"]);
    }

    if selected.is_empty() {
        return Vec::new();
    }

    definitions()
        .into_iter()
        .filter(|def| selected.contains(&def.function.name))
        .collect()
}

fn has_any(text: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|kw| text.contains(kw))
}

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        tool(
            "run_shell",
            "Run a bash shell command on the user's Linux system and return stdout/stderr.",
            json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "cwd": { "type": "string" },
                    "timeout_secs": { "type": "integer" }
                },
                "required": ["command"]
            }),
        ),
        tool(
            "read_file",
            "Read lines from a local file (~ supported).",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "start_line": { "type": "integer" },
                    "end_line": { "type": "integer" }
                },
                "required": ["path"]
            }),
        ),
        tool(
            "write_file",
            "Create, overwrite, or append to a local file.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" },
                    "append": { "type": "boolean" }
                },
                "required": ["path", "content"]
            }),
        ),
        tool(
            "edit_file",
            "Replace an exact target text block inside an existing file.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "target": { "type": "string" },
                    "replacement": { "type": "string" }
                },
                "required": ["path", "target", "replacement"]
            }),
        ),
        tool(
            "list_dir",
            "List files and subdirectories in a directory (defaults to ~).",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                }
            }),
        ),
        tool(
            "search_files",
            "Search for files by filename pattern or text content inside a directory.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "query": { "type": "string" },
                    "filename_pattern": { "type": "string" }
                }
            }),
        ),
        tool(
            "manage_path",
            "Open a folder/file in file manager, create directory, move, copy, or delete (moves to Trash by default; set permanent=true only if explicitly requested).",
            json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["open", "mkdir", "move", "copy", "delete"] },
                    "path": { "type": "string" },
                    "destination": { "type": "string" },
                    "permanent": { "type": "boolean" }
                },
                "required": ["action", "path"]
            }),
        ),
        tool(
            "open_url",
            "Open any URL in a specific browser (firefox, google-chrome, chromium, brave, or default).",
            json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string" },
                    "browser": { "type": "string" }
                },
                "required": ["url"]
            }),
        ),
        tool(
            "browser_search",
            "Open a search page on YouTube, Google, GitHub, DuckDuckGo, Wikipedia, or StackOverflow in a browser.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "site": { "type": "string", "enum": ["youtube", "google", "github", "duckduckgo", "wikipedia", "stackoverflow"] },
                    "browser": { "type": "string" }
                },
                "required": ["query"]
            }),
        ),
        tool(
            "play_youtube",
            "Find the top matching YouTube video for a query and open it directly in the browser.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "browser": { "type": "string" }
                },
                "required": ["query"]
            }),
        ),
        tool(
            "launch_app",
            "Launch a desktop application (e.g. firefox, chrome, vscode, terminal, files, vlc, spotify).",
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "args": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["name"]
            }),
        ),
        tool(
            "list_processes",
            "List running processes sorted by memory usage, optionally filtered by name.",
            json!({
                "type": "object",
                "properties": {
                    "filter": { "type": "string" },
                    "limit": { "type": "integer" }
                }
            }),
        ),
        tool(
            "kill_process",
            "Terminate a running process by PID or name.",
            json!({
                "type": "object",
                "properties": {
                    "pid": { "type": "integer" },
                    "name": { "type": "string" },
                    "force": { "type": "boolean" }
                }
            }),
        ),
        tool(
            "get_system_info",
            "Get hostname, uptime, CPU load, RAM, disk, battery, IP, and audio volume.",
            json!({ "type": "object", "properties": {} }),
        ),
        tool(
            "set_volume",
            "Control system audio volume (get, set, up, down, mute, unmute, toggle_mute).",
            json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["get", "set", "up", "down", "mute", "unmute", "toggle_mute"] },
                    "value": { "type": "integer" }
                },
                "required": ["action"]
            }),
        ),
        tool(
            "set_brightness",
            "Control screen brightness (get, set, up, down).",
            json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["get", "set", "up", "down"] },
                    "value": { "type": "integer" }
                },
                "required": ["action"]
            }),
        ),
        tool(
            "send_notification",
            "Show a desktop notification.",
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "body": { "type": "string" },
                    "urgency": { "type": "string", "enum": ["low", "normal", "critical"] }
                },
                "required": ["title"]
            }),
        ),
        tool(
            "power_action",
            "Lock screen or sleep/reboot/shutdown the computer.",
            json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["lock", "sleep", "reboot", "shutdown"] }
                },
                "required": ["action"]
            }),
        ),
        tool(
            "clipboard_read",
            "Read current text from the system clipboard.",
            json!({ "type": "object", "properties": {} }),
        ),
        tool(
            "clipboard_write",
            "Write text to the system clipboard.",
            json!({
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"]
            }),
        ),
        tool(
            "type_text",
            "Type text into the currently focused window.",
            json!({
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"]
            }),
        ),
        tool(
            "press_key",
            "Press a key or shortcut (e.g. Return, ctrl+c, alt+Tab, XF86AudioPlay).",
            json!({
                "type": "object",
                "properties": { "keys": { "type": "string" } },
                "required": ["keys"]
            }),
        ),
        tool(
            "mouse_action",
            "Move, click, or scroll the mouse cursor.",
            json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["move", "click", "right_click", "double_click", "scroll_up", "scroll_down"] },
                    "x": { "type": "integer" },
                    "y": { "type": "integer" },
                    "amount": { "type": "integer" }
                },
                "required": ["action"]
            }),
        ),
        tool(
            "take_screenshot",
            "Capture a screenshot of the screen or active window and save it.",
            json!({
                "type": "object",
                "properties": {
                    "mode": { "type": "string", "enum": ["full", "window"] },
                    "filename": { "type": "string" }
                }
            }),
        ),
        tool(
            "web_fetch",
            "Fetch and extract readable text from a web page URL.",
            json!({
                "type": "object",
                "properties": { "url": { "type": "string" } },
                "required": ["url"]
            }),
        ),
        tool(
            "web_search",
            "Search the web for live information and return top results.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer" }
                },
                "required": ["query"]
            }),
        ),
        tool(
            "set_timer",
            "Set a countdown timer in seconds with an optional label.",
            json!({
                "type": "object",
                "properties": {
                    "seconds": { "type": "integer" },
                    "label": { "type": "string" }
                },
                "required": ["seconds"]
            }),
        ),
        tool(
            "list_timers",
            "List currently active timers.",
            json!({ "type": "object", "properties": {} }),
        ),
        tool(
            "cancel_timer",
            "Cancel an active timer by its ID.",
            json!({
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"]
            }),
        ),
        tool(
            "save_memory",
            "Save a persistent fact, user preference, or note to long-term memory across restarts.",
            json!({
                "type": "object",
                "properties": {
                    "content": { "type": "string" },
                    "category": { "type": "string", "enum": ["fact", "preference", "project", "note"] }
                },
                "required": ["content"]
            }),
        ),
        tool(
            "search_memory",
            "Search or list saved long-term memories.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                }
            }),
        ),
        tool(
            "forget_memory",
            "Remove a saved memory by its numeric ID or matching text query.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "integer" },
                    "query": { "type": "string" }
                }
            }),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn tool_definitions_have_unique_names() {
        let defs = definitions();
        assert_eq!(defs.len(), 32);
        let names: HashSet<&str> = defs.iter().map(|d| d.function.name).collect();
        assert_eq!(names.len(), defs.len());
    }

    #[test]
    fn destructive_shell_patterns_detected() {
        assert!(shell::is_destructive_shell("rm -rf /").is_some());
        assert!(shell::is_destructive_shell("sudo shutdown now").is_some());
        assert!(shell::is_destructive_shell("ls -la ~/Downloads").is_none());
    }

    #[test]
    fn youtube_video_id_extracted() {
        let html = r#"{"videoRenderer":{"videoId":"dQw4w9WgXcQ"}}"#;
        assert_eq!(
            browser::extract_first_video_id(html).as_deref(),
            Some("dQw4w9WgXcQ")
        );
    }

    #[test]
    fn html_stripping_removes_scripts_and_tags() {
        let html = "<html><head><style>body{}</style><script>bad()</script></head><body><h1>Hello</h1><p>Zara &amp; You</p></body></html>";
        assert_eq!(web::strip_html(html), "Hello Zara & You");
    }

    #[tokio::test]
    async fn filesystem_write_edit_read_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("zara-fs-test-{}", std::process::id()));
        let path_str = tmp.to_string_lossy().into_owned();

        let write_res = filesystem::execute_write_file(json!({
            "path": path_str,
            "content": "line one\nline two\n"
        }))
        .await
        .unwrap();
        assert!(write_res.contains("Wrote"));

        filesystem::execute_edit_file(json!({
            "path": path_str,
            "target": "line two",
            "replacement": "line 2 updated"
        }))
        .await
        .unwrap();

        let read_res = filesystem::execute_read_file(json!({
            "path": path_str,
            "start_line": 1,
            "end_line": 5
        }))
        .await
        .unwrap();
        assert!(read_res.contains("2: line 2 updated"));

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn resolve_path_expands_tilde_and_matches_plural_folder() {
        if let Ok(home) = filesystem::home_dir()
            && home.join("Downloads").is_dir()
        {
            let resolved = filesystem::resolve_path("~/Download").unwrap();
            assert_eq!(resolved, home.join("Downloads"));
        }
    }

    #[test]
    fn memory_save_search_forget_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("zara-mem-test-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&tmp);

        let saved = memory::save_memory_at(
            &tmp,
            "User prefers VS Code".to_string(),
            "preference".to_string(),
        )
        .unwrap();
        assert!(saved.contains("Saved memory #1"));

        let dup = memory::save_memory_at(
            &tmp,
            "user prefers vs code".to_string(),
            "preference".to_string(),
        )
        .unwrap();
        assert!(dup.contains("already saved as #1"));

        let found = memory::search_memory_at(&tmp, Some("VS Code")).unwrap();
        assert!(found.contains("#1 [preference]: User prefers VS Code"));

        let removed = memory::forget_memory_at(&tmp, Some(1), None).unwrap();
        assert!(removed.contains("Removed 1"));

        let empty = memory::search_memory_at(&tmp, None).unwrap();
        assert_eq!(empty, "No saved memories yet.");

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn move_to_trash_dir_moves_file_and_writes_trashinfo() {
        let base = std::env::temp_dir().join(format!("zara-trash-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let src_file = base.join("sample.txt");
        std::fs::write(&src_file, "hello trash").unwrap();
        let trash_root = base.join("Trash");

        let res = filesystem::move_to_trash_dir(&src_file, &trash_root).unwrap();
        assert!(res.contains("Moved"));
        assert!(!src_file.exists());
        assert!(trash_root.join("files/sample.txt").exists());
        assert!(trash_root.join("info/sample.txt.trashinfo").exists());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn select_definitions_filters_by_intent() {
        let browser_tools = select_definitions("mozilla browser open করে youtube এ sad song search দাও");
        assert!(!browser_tools.is_empty() && browser_tools.len() <= 8);
        assert!(browser_tools.iter().any(|t| t.function.name == "browser_search"));
        assert!(!browser_tools.iter().any(|t| t.function.name == "power_action"));

        let chat_tools = select_definitions("তুমি কেমন আছো?");
        assert!(chat_tools.is_empty());
    }
}
