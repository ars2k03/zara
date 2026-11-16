use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::tools;
use crate::tools::filesystem;
use crate::tools::shell;

fn resolve_browser_binary(browser: Option<&str>) -> &'static str {
    let normalized = browser.unwrap_or_default().trim().to_lowercase();
    let candidate = match normalized.as_str() {
        "" | "default" | "system" => return "xdg-open",
        "mozilla" | "firefox" | "mozilla firefox" => "firefox",
        "chrome" | "google chrome" | "google-chrome" => "google-chrome",
        "chromium" => "chromium",
        "brave" | "brave-browser" => "brave-browser",
        _ => "xdg-open",
    };

    if shell::command_exists(candidate) {
        candidate
    } else if candidate == "google-chrome" && shell::command_exists("chromium") {
        "chromium"
    } else {
        "xdg-open"
    }
}

fn normalize_url(raw: &str) -> AppResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::from("URL cannot be empty."));
    }
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    let parsed = url::Url::parse(&with_scheme)?;
    Ok(parsed.into())
}

fn launch_url(url: &str, browser: Option<&str>) -> AppResult<String> {
    let bin = resolve_browser_binary(browser);
    let args = vec![url.to_string()];
    shell::spawn_detached(bin, &args)?;
    Ok(bin.to_string())
}

pub async fn execute_open_url(args: Value) -> AppResult<String> {
    let raw_url = tools::req_str(&args, "url")?;
    let browser = tools::opt_str(&args, "browser");
    let trimmed = raw_url.trim();
    if trimmed == "~" || trimmed.starts_with("~/") || trimmed.starts_with('/') {
        let resolved = filesystem::resolve_path(trimmed)?;
        let target = resolved.to_string_lossy().into_owned();
        let opener = if browser.is_none() { "xdg-open" } else { resolve_browser_binary(browser.as_deref()) };
        shell::spawn_detached(opener, std::slice::from_ref(&target))?;
        return Ok(format!("Opened {target} in {opener}."));
    }
    let url = normalize_url(&raw_url)?;
    let launched = launch_url(&url, browser.as_deref())?;
    Ok(format!("Opened {url} in {launched}."))
}

pub async fn execute_browser_search(args: Value) -> AppResult<String> {
    let query = tools::req_str(&args, "query")?;
    let site = tools::opt_str(&args, "site")
        .unwrap_or_else(|| "google".to_string())
        .to_lowercase();
    let browser = tools::opt_str(&args, "browser");
    let encoded = urlencoding::encode(query.trim());

    let url = match site.as_str() {
        "youtube" | "yt" => format!("https://www.youtube.com/results?search_query={encoded}"),
        "github" => format!("https://github.com/search?q={encoded}"),
        "duckduckgo" | "ddg" => format!("https://duckduckgo.com/?q={encoded}"),
        "wikipedia" | "wiki" => format!("https://en.wikipedia.org/w/index.php?search={encoded}"),
        "stackoverflow" | "so" => format!("https://stackoverflow.com/search?q={encoded}"),
        _ => format!("https://www.google.com/search?q={encoded}"),
    };

    let launched = launch_url(&url, browser.as_deref())?;
    Ok(format!("Searched {site} for \"{query}\" in {launched} ({url})."))
}

pub async fn execute_play_youtube(args: Value) -> AppResult<String> {
    let query = tools::req_str(&args, "query")?;
    let browser = tools::opt_str(&args, "browser");

    let video_id = find_youtube_video_id(&query).await;
    let url = match video_id {
        Some(id) => format!("https://www.youtube.com/watch?v={id}"),
        None => format!(
            "https://www.youtube.com/results?search_query={}",
            urlencoding::encode(query.trim())
        ),
    };

    let launched = launch_url(&url, browser.as_deref())?;
    Ok(format!("Opened YouTube for \"{query}\" in {launched} ({url})."))
}

async fn find_youtube_video_id(query: &str) -> Option<String> {
    if shell::command_exists("yt-dlp") {
        let args = vec![
            "--get-id".to_string(),
            format!("ytsearch1:{query}"),
        ];
        if let Ok(output) = shell::run_program("yt-dlp", &args, None, 10).await
            && let Some(id) = output.stdout.lines().next().map(str::trim)
            && is_valid_video_id(id)
        {
            return Some(id.to_string());
        }
    }

    let search_url = format!(
        "https://www.youtube.com/results?search_query={}",
        urlencoding::encode(query.trim())
    );
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36")
        .build()
        .ok()?;
    let html = client.get(search_url).send().await.ok()?.text().await.ok()?;
    extract_first_video_id(&html)
}

pub(crate) fn extract_first_video_id(html: &str) -> Option<String> {
    let marker = "\"videoId\":\"";
    let mut rest = html;
    while let Some(pos) = rest.find(marker) {
        let after = &rest[pos + marker.len()..];
        if let Some(end) = after.find('"') {
            let candidate = &after[..end];
            if is_valid_video_id(candidate) {
                return Some(candidate.to_string());
            }
            rest = &after[end..];
        } else {
            break;
        }
    }
    None
}

fn is_valid_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}
