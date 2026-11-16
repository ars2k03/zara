use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tauri::AppHandle;

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::tools;
use crate::tools::shell;

const MAX_READ_LINES: usize = 300;
const MAX_LIST_ENTRIES: usize = 200;
const MAX_SEARCH_MATCHES: usize = 100;

pub fn resolve_path(raw: &str) -> AppResult<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::from("Path cannot be empty."));
    }

    let expanded = if trimmed == "~" {
        home_dir()?
    } else if let Some(rest) = trimmed.strip_prefix("~/") {
        home_dir()?.join(rest)
    } else {
        PathBuf::from(trimmed)
    };

    let resolved = if expanded.is_absolute() {
        expanded
    } else {
        env::current_dir()?.join(expanded)
    };

    if resolved.exists() {
        return Ok(resolved);
    }

    Ok(match_existing_sibling(&resolved).unwrap_or(resolved))
}

fn match_existing_sibling(path: &Path) -> Option<PathBuf> {
    let parent = path.parent()?;
    let target = path.file_name()?.to_str()?.to_lowercase();
    let entries = fs::read_dir(parent).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let lower = name.to_string_lossy().to_lowercase();
        if lower == target || lower == format!("{target}s") {
            return Some(entry.path());
        }
    }
    None
}

pub fn home_dir() -> AppResult<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| AppError::from("HOME environment variable is not set."))
}

pub async fn execute_read_file(args: Value) -> AppResult<String> {
    let path = resolve_path(&tools::req_str(&args, "path")?)?;
    let start = args.get("start_line").and_then(|v| v.as_u64()).unwrap_or(1).max(1) as usize;
    let end = args
        .get("end_line")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(start + MAX_READ_LINES - 1);

    tokio::task::spawn_blocking(move || {
        let content = fs::read_to_string(&path)
            .map_err(|err| AppError::Message(format!("Failed to read {}: {err}", path.display())))?;
        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();
        if total == 0 {
            return Ok(format!("{} is empty.", path.display()));
        }
        let from = start.saturating_sub(1).min(total);
        let to = end.min(from + MAX_READ_LINES).min(total);
        let numbered: Vec<String> = lines[from..to]
            .iter()
            .enumerate()
            .map(|(idx, line)| format!("{}: {line}", from + idx + 1))
            .collect();
        Ok(format!(
            "File: {} (lines {}-{} of {total})\n{}",
            path.display(),
            from + 1,
            to,
            numbered.join("\n")
        ))
    })
    .await
    .map_err(|err| AppError::Message(err.to_string()))?
}

pub async fn execute_write_file(args: Value) -> AppResult<String> {
    let path = resolve_path(&tools::req_str(&args, "path")?)?;
    let content = tools::req_str(&args, "content")?;
    let append = args.get("append").and_then(|v| v.as_bool()).unwrap_or(false);

    tokio::task::spawn_blocking(move || {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if append {
            let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
            file.write_all(content.as_bytes())?;
            Ok(format!("Appended {} bytes to {}.", content.len(), path.display()))
        } else {
            fs::write(&path, content.as_bytes())?;
            Ok(format!("Wrote {} bytes to {}.", content.len(), path.display()))
        }
    })
    .await
    .map_err(|err| AppError::Message(err.to_string()))?
}

pub async fn execute_edit_file(args: Value) -> AppResult<String> {
    let path = resolve_path(&tools::req_str(&args, "path")?)?;
    let target = tools::req_str(&args, "target")?;
    let replacement = tools::req_str(&args, "replacement")?;

    tokio::task::spawn_blocking(move || {
        let original = fs::read_to_string(&path)
            .map_err(|err| AppError::Message(format!("Failed to read {}: {err}", path.display())))?;
        if !original.contains(&target) {
            return Err(AppError::Message(format!(
                "Target text was not found in {}.",
                path.display()
            )));
        }
        let updated = original.replacen(&target, &replacement, 1);
        fs::write(&path, updated.as_bytes())?;
        Ok(format!("Updated {}.", path.display()))
    })
    .await
    .map_err(|err| AppError::Message(err.to_string()))?
}

pub async fn execute_list_dir(args: Value) -> AppResult<String> {
    let raw = tools::opt_str(&args, "path").unwrap_or_else(|| "~".to_string());
    let path = resolve_path(&raw)?;

    tokio::task::spawn_blocking(move || {
        let mut entries = Vec::new();
        for entry in fs::read_dir(&path)
            .map_err(|err| AppError::Message(format!("Failed to list {}: {err}", path.display())))?
        {
            let entry = entry?;
            let meta = entry.metadata()?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let kind = if meta.is_dir() { "dir" } else { "file" };
            entries.push((kind, name, meta.len()));
        }
        entries.sort_by(|a, b| a.0.cmp(b.0).then_with(|| a.1.cmp(&b.1)));
        let total = entries.len();
        let lines: Vec<String> = entries
            .into_iter()
            .take(MAX_LIST_ENTRIES)
            .map(|(kind, name, size)| {
                if kind == "dir" {
                    format!("[dir]  {name}/")
                } else {
                    format!("[file] {name} ({size} B)")
                }
            })
            .collect();
        Ok(format!("Directory: {} ({total} items)\n{}", path.display(), lines.join("\n")))
    })
    .await
    .map_err(|err| AppError::Message(err.to_string()))?
}

pub async fn execute_search_files(args: Value) -> AppResult<String> {
    let raw_path = tools::opt_str(&args, "path").unwrap_or_else(|| "~".to_string());
    let root = resolve_path(&raw_path)?;
    let query = tools::opt_str(&args, "query");
    let filename_pattern = tools::opt_str(&args, "filename_pattern");

    if query.is_none() && filename_pattern.is_none() {
        return Err(AppError::from("Provide either `query` or `filename_pattern`."));
    }

    if let Some(ref text_query) = query
        && shell::command_exists("rg")
    {
        let mut rg_args = vec![
            "-n".to_string(),
            "--no-heading".to_string(),
            "--max-count".to_string(),
            "5".to_string(),
        ];
        if let Some(ref glob) = filename_pattern {
            rg_args.push("-g".to_string());
            rg_args.push(glob.clone());
        }
        rg_args.push(text_query.clone());
        rg_args.push(root.to_string_lossy().into_owned());
        let output = shell::run_program("rg", &rg_args, None, 15).await?;
        let trimmed = output.stdout.trim();
        return Ok(if trimmed.is_empty() {
            "No matches found.".to_string()
        } else {
            shell::truncate_output(trimmed, 10_000)
        });
    }

    tokio::task::spawn_blocking(move || {
        let mut matches = Vec::new();
        walk_search(
            &root,
            query.as_deref(),
            filename_pattern.as_deref(),
            &mut matches,
        );
        if matches.is_empty() {
            Ok("No matches found.".to_string())
        } else {
            Ok(matches.join("\n"))
        }
    })
    .await
    .map_err(|err| AppError::Message(err.to_string()))?
}

fn walk_search(
    dir: &Path,
    query: Option<&str>,
    filename_pattern: Option<&str>,
    matches: &mut Vec<String>,
) {
    if matches.len() >= MAX_SEARCH_MATCHES {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let pattern_lower = filename_pattern.map(|s| s.trim_matches('*').to_lowercase());
    let query_lower = query.map(|s| s.to_lowercase());

    for entry in entries.flatten() {
        if matches.len() >= MAX_SEARCH_MATCHES {
            break;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".git" || name == "node_modules" || name == "target" {
            continue;
        }
        if path.is_dir() {
            walk_search(&path, query, filename_pattern, matches);
            continue;
        }
        if let Some(ref pat) = pattern_lower
            && !name.to_lowercase().contains(pat)
        {
            continue;
        }
        if let Some(ref q) = query_lower {
            if let Ok(content) = fs::read_to_string(&path) {
                for (idx, line) in content.lines().enumerate() {
                    if line.to_lowercase().contains(q) {
                        matches.push(format!("{}:{}: {}", path.display(), idx + 1, line.trim()));
                        if matches.len() >= MAX_SEARCH_MATCHES {
                            break;
                        }
                    }
                }
            }
        } else {
            matches.push(path.display().to_string());
        }
    }
}

pub async fn execute_manage_path(app: &AppHandle, state: &AppState, args: Value) -> AppResult<String> {
    let action = tools::req_str(&args, "action")?;
    let path = resolve_path(&tools::req_str(&args, "path")?)?;
    let permanent = args.get("permanent").and_then(|v| v.as_bool()).unwrap_or(false);
    let destination = tools::opt_str(&args, "destination")
        .map(|raw| resolve_path(&raw))
        .transpose()?;

    if action == "open" {
        if !path.exists() {
            return Err(AppError::Message(format!("Path does not exist: {}", path.display())));
        }
        let opener = if shell::command_exists("xdg-open") {
            "xdg-open"
        } else if shell::command_exists("thunar") {
            "thunar"
        } else {
            return Err(AppError::from("No file manager or xdg-open found."));
        };
        let target = path.to_string_lossy().into_owned();
        shell::spawn_detached(opener, std::slice::from_ref(&target))?;
        return Ok(format!("Opened {} in {opener}.", path.display()));
    }

    if action == "delete" {
        if !path.exists() {
            return Err(AppError::Message(format!("Path does not exist: {}", path.display())));
        }
        if permanent {
            tools::confirm_action(
                app,
                state,
                "স্থায়ীভাবে ডিলিট কনফার্মেশন".to_string(),
                format!("Trash-এ না পাঠিয়ে স্থায়ীভাবে ডিলিট করতে চান: {}?", path.display()),
            )
            .await?;
            return tokio::task::spawn_blocking(move || {
                if path.is_dir() {
                    fs::remove_dir_all(&path)?;
                } else {
                    fs::remove_file(&path)?;
                }
                Ok(format!("Permanently deleted {}.", path.display()))
            })
            .await
            .map_err(|err| AppError::Message(err.to_string()))?;
        }
        return move_to_trash(&path).await;
    }

    tokio::task::spawn_blocking(move || match action.as_str() {
        "mkdir" => {
            fs::create_dir_all(&path)?;
            Ok(format!("Created directory {}.", path.display()))
        }
        "move" => {
            let dest = destination.ok_or_else(|| AppError::from("`destination` is required for move."))?;
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(&path, &dest)?;
            Ok(format!("Moved {} to {}.", path.display(), dest.display()))
        }
        "copy" => {
            let dest = destination.ok_or_else(|| AppError::from("`destination` is required for copy."))?;
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            if path.is_dir() {
                copy_dir_recursive(&path, &dest)?;
            } else {
                fs::copy(&path, &dest)?;
            }
            Ok(format!("Copied {} to {}.", path.display(), dest.display()))
        }
        other => Err(AppError::Message(format!("Unsupported manage_path action: {other}"))),
    })
    .await
    .map_err(|err| AppError::Message(err.to_string()))?
}

async fn move_to_trash(path: &Path) -> AppResult<String> {
    let target = path.to_string_lossy().into_owned();
    if shell::command_exists("gio") {
        let out = shell::run_program("gio", &["trash".into(), target.clone()], None, 10).await?;
        if out.exit_code == 0 {
            return Ok(format!("Moved {} to Trash.", path.display()));
        }
    }
    if shell::command_exists("trash-put") {
        let out = shell::run_program("trash-put", std::slice::from_ref(&target), None, 10).await?;
        if out.exit_code == 0 {
            return Ok(format!("Moved {} to Trash.", path.display()));
        }
    }

    let path_buf = path.to_path_buf();
    let trash_root = trash_dir()?;
    tokio::task::spawn_blocking(move || move_to_trash_dir(&path_buf, &trash_root))
        .await
        .map_err(|err| AppError::Message(err.to_string()))?
}

fn trash_dir() -> AppResult<PathBuf> {
    if let Ok(xdg) = env::var("XDG_DATA_HOME")
        && !xdg.trim().is_empty()
    {
        return Ok(PathBuf::from(xdg).join("Trash"));
    }
    Ok(home_dir()?.join(".local").join("share").join("Trash"))
}

pub(crate) fn move_to_trash_dir(path: &Path, trash_root: &Path) -> AppResult<String> {
    let files_dir = trash_root.join("files");
    let info_dir = trash_root.join("info");
    fs::create_dir_all(&files_dir)?;
    fs::create_dir_all(&info_dir)?;

    let base_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| AppError::from("Invalid file name for Trash."))?;

    let mut dest_name = base_name.to_string();
    let mut counter = 1u32;
    while files_dir.join(&dest_name).exists() || info_dir.join(format!("{dest_name}.trashinfo")).exists() {
        dest_name = format!("{base_name}.{counter}");
        counter += 1;
    }

    let dest_path = files_dir.join(&dest_name);
    let info_path = info_dir.join(format!("{dest_name}.trashinfo"));
    let info_content = format!(
        "[Trash Info]\nPath={}\nDeletionDate=2026-01-01T00:00:00\n",
        path.display()
    );
    fs::write(&info_path, info_content)?;
    if let Err(err) = fs::rename(path, &dest_path) {
        let _ = fs::remove_file(&info_path);
        return Err(AppError::from(err));
    }
    Ok(format!("Moved {} to Trash.", path.display()))
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> AppResult<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
