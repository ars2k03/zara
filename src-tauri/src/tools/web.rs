use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::tools;
use crate::tools::shell;

const MAX_WEB_BYTES: usize = 10_000;

fn http_client() -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36")
        .build()
        .map_err(AppError::from)
}

pub async fn execute_web_fetch(args: Value) -> AppResult<String> {
    let raw_url = tools::req_str(&args, "url")?;
    let url = if raw_url.contains("://") {
        raw_url
    } else {
        format!("https://{raw_url}")
    };

    let client = http_client()?;
    let response = client.get(&url).send().await?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        return Err(AppError::Message(format!("HTTP {status} fetching {url}")));
    }

    let cleaned = strip_html(&body);
    Ok(shell::truncate_output(&cleaned, MAX_WEB_BYTES))
}

#[derive(Deserialize)]
struct DdgResponse {
    #[serde(rename = "Heading")]
    heading: Option<String>,
    #[serde(rename = "AbstractText")]
    abstract_text: Option<String>,
    #[serde(rename = "AbstractURL")]
    abstract_url: Option<String>,
    #[serde(rename = "RelatedTopics")]
    related_topics: Option<Vec<DdgTopic>>,
}

#[derive(Deserialize)]
struct DdgTopic {
    #[serde(rename = "Text")]
    text: Option<String>,
    #[serde(rename = "FirstURL")]
    first_url: Option<String>,
}

pub async fn execute_web_search(args: Value) -> AppResult<String> {
    let query = tools::req_str(&args, "query")?;
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(5).min(10) as usize;
    let client = http_client()?;

    let html_url = format!(
        "https://html.duckduckgo.com/html/?q={}",
        urlencoding::encode(query.trim())
    );
    if let Ok(resp) = client.get(&html_url).send().await
        && resp.status().is_success()
        && let Ok(html) = resp.text().await
    {
        let results = parse_ddg_html(&html, limit);
        if !results.is_empty() {
            return Ok(results.join("\n\n"));
        }
    }

    let api_url = format!(
        "https://api.duckduckgo.com/?q={}&format=json&no_html=1&skip_disambig=1",
        urlencoding::encode(query.trim())
    );
    let data: DdgResponse = client.get(api_url).send().await?.json().await?;
    let mut lines = Vec::new();
    if let Some(text) = data.abstract_text.filter(|s| !s.is_empty()) {
        let heading = data.heading.unwrap_or_else(|| query.clone());
        let src = data.abstract_url.unwrap_or_default();
        lines.push(format!("{heading}: {text} ({src})"));
    }
    if let Some(topics) = data.related_topics {
        for topic in topics.into_iter().take(limit) {
            if let Some(text) = topic.text {
                let url = topic.first_url.unwrap_or_default();
                lines.push(format!("- {text} ({url})"));
            }
        }
    }

    if lines.is_empty() {
        Ok(format!("No web summary found for \"{query}\"."))
    } else {
        Ok(lines.join("\n"))
    }
}

fn parse_ddg_html(html: &str, limit: usize) -> Vec<String> {
    let mut results = Vec::new();
    let marker = "class=\"result__a\"";
    let mut rest = html;
    while results.len() < limit {
        let Some(pos) = rest.find(marker) else {
            break;
        };
        let chunk = &rest[pos..];
        let href = extract_attr(chunk, "href").unwrap_or_default();
        let title_raw = extract_tag_text(chunk).unwrap_or_default();
        let snippet = chunk
            .find("class=\"result__snippet\"")
            .and_then(|s_pos| extract_tag_text(&chunk[s_pos..]))
            .unwrap_or_default();
        let title = decode_entities(&title_raw).trim().to_string();
        let snip = decode_entities(&snippet).trim().to_string();
        if !title.is_empty() {
            results.push(format!("{title}\nURL: {href}\n{snip}"));
        }
        rest = &rest[pos + marker.len()..];
    }
    results
}

fn extract_attr(chunk: &str, attr: &str) -> Option<String> {
    let needle = format!("{attr}=\"");
    let start = chunk.find(&needle)? + needle.len();
    let end = chunk[start..].find('"')?;
    Some(chunk[start..start + end].to_string())
}

fn extract_tag_text(chunk: &str) -> Option<String> {
    let open = chunk.find('>')? + 1;
    let close = chunk[open..].find("</")?;
    Some(strip_html(&chunk[open..open + close]))
}

pub(crate) fn strip_html(html: &str) -> String {
    let without_scripts = strip_tag_block(html, "script");
    let without_styles = strip_tag_block(&without_scripts, "style");

    let mut out = String::with_capacity(without_styles.len());
    let mut in_tag = false;
    for ch in without_styles.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }

    let decoded = decode_entities(&out);
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_tag_block(input: &str, tag: &str) -> String {
    let open_pat = format!("<{tag}");
    let close_pat = format!("</{tag}>");
    let mut result = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.to_lowercase().find(&open_pat) {
        result.push_str(&rest[..start]);
        let after_open = &rest[start..];
        if let Some(end_rel) = after_open.to_lowercase().find(&close_pat) {
            rest = &after_open[end_rel + close_pat.len()..];
        } else {
            return result;
        }
    }
    result.push_str(rest);
    result
}

fn decode_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}
