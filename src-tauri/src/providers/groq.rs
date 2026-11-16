use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::AppHandle;

use crate::config;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::tools::{self, ToolDefinition};

const GROQ_CHAT_URL: &str = "https://api.groq.com/openai/v1/chat/completions";
const LLM_MODEL: &str = "openai/gpt-oss-120b";
const MAX_TOOL_ROUNDS: usize = 8;
const MAX_HISTORY_MESSAGES: usize = 6;

const SYSTEM_PROMPT: &str = "\
You are Zara, a helpful voice assistant with full access to the user's Linux computer through your tools.

Language:
- Reply mainly in Bangla, written in Bangla script.
- Use natural Bangla–English code-switching: keep words in English when they sound clearer or more natural that way (e.g. AI, API, app, file, Firefox, YouTube, okay, wifi, laptop).
- Do not write full answers only in English.
- Do not write Bangla in Latin letters.

Capabilities & Execution:
- You can execute shell commands, read/write/edit/search/manage files, open URLs and search or play videos on YouTube in specific browsers (such as Firefox/Mozilla, Chrome, Chromium), launch/kill desktop apps, check system stats, control volume/brightness/power, read/write clipboard, automate keyboard/mouse, take screenshots, search/fetch the web, set timers, and remember/forget facts with long-term memory (`save_memory`, `search_memory`, `forget_memory`).
- Whenever the user shares important personal details, preferences, or asks you to remember something, call `save_memory` to store it persistently. When asked to forget something, call `forget_memory`.
- Whenever the user asks you to perform an action on their computer or look up live info, call the appropriate tool(s) immediately instead of saying you cannot do it.
- To open a local folder or file in the file manager (e.g. Downloads, Documents, Desktop), use `manage_path` with `action: \"open\"` and `path` (e.g. `~/Downloads`).
- To delete files or folders, ALWAYS use `manage_path` with `action: \"delete\"` (which moves them safely to Trash/Recycle Bin) instead of running `rm` via `run_shell`.
- Report the actual outcome of the tool concisely. If a tool fails, state the real reason briefly.

Style:
- Keep replies short and conversational — they will be spoken aloud.
- No emoji, emoticons, or markdown.
- Prefer plain spoken sentences over lists or headings.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ToolCall {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ToolCallFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GroqMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

impl From<ChatMessage> for GroqMessage {
    fn from(msg: ChatMessage) -> Self {
        Self {
            role: msg.role,
            content: Some(msg.content),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'static str,
    messages: &'a [GroqMessage],
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<&'a [ToolDefinition]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<&'static str>,
    temperature: f32,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: GroqMessage,
}

pub async fn reply(
    app: &AppHandle,
    state: &AppState,
    history: Vec<ChatMessage>,
) -> AppResult<String> {
    let api_key = config::require("GROQ_API_KEY")?;
    let memory_block = tools::memory::format_for_system_prompt().await;
    let system_prompt = if memory_block.is_empty() {
        SYSTEM_PROMPT.to_string()
    } else {
        format!("{SYSTEM_PROMPT}\n\nSaved User Memory:\n{memory_block}")
    };

    let latest_user = history
        .iter()
        .rev()
        .find(|msg| msg.role == "user")
        .map(|msg| msg.content.as_str())
        .unwrap_or("");
    let definitions = tools::select_definitions(latest_user);

    let history_start = history.len().saturating_sub(MAX_HISTORY_MESSAGES);
    let mut messages = vec![GroqMessage {
        role: "system".into(),
        content: Some(system_prompt),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    }];
    messages.extend(history.into_iter().skip(history_start).map(GroqMessage::from));

    let client = reqwest::Client::new();

    for _round in 0..MAX_TOOL_ROUNDS {
        let response = match send_chat_request(&client, &api_key, &messages, &definitions).await {
            Ok(resp) => resp,
            Err(err) => {
                eprintln!("Groq request error: {err}");
                return Err(AppError::from(
                    "দুঃখিত স্যার, আমি এই মুহূর্তে এই কাজটি করতে পারছি না। একটু পরে আবার চেষ্টা করুন।",
                ));
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            eprintln!("Groq LLM failed ({status}): {body}");
            return Err(AppError::from(friendly_http_error(status.as_u16())));
        }

        let payload: ChatResponse = match response.json().await {
            Ok(data) => data,
            Err(err) => {
                eprintln!("Groq response parse error: {err}");
                return Err(AppError::from(
                    "দুঃখিত স্যার, আমি এই মুহূর্তে এই কাজটি করতে পারছি না। একটু পরে আবার চেষ্টা করুন।",
                ));
            }
        };
        let assistant = payload
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message)
            .ok_or_else(|| {
                AppError::from(
                    "দুঃখিত স্যার, আমি এই মুহূর্তে উত্তর দিতে পারছি না। একটু পরে আবার চেষ্টা করুন।",
                )
            })?;

        if let Some(calls) = assistant.tool_calls.clone().filter(|c| !c.is_empty()) {
            messages.push(assistant);
            for call in calls {
                let parsed_args: Value =
                    serde_json::from_str(&call.function.arguments).unwrap_or_else(|_| Value::Object(Default::default()));
                let outcome = match tools::execute(app, state, &call.function.name, parsed_args).await {
                    Ok(output) => output,
                    Err(err) => format!("Error: {err}"),
                };
                messages.push(GroqMessage {
                    role: "tool".into(),
                    content: Some(outcome),
                    tool_calls: None,
                    tool_call_id: Some(call.id),
                    name: Some(call.function.name),
                });
            }
            continue;
        }

        let content = assistant
            .content
            .map(|text| sanitize_spoken(&text))
            .filter(|text| !text.is_empty())
            .ok_or_else(|| {
                AppError::from(
                    "দুঃখিত স্যার, আমি এই মুহূর্তে উত্তর দিতে পারছি না। একটু পরে আবার চেষ্টা করুন।",
                )
            })?;

        return Ok(content);
    }

    Err(AppError::from(
        "দুঃখিত স্যার, কাজটি করতে বেশি সময় লাগছে। একটু পরে আবার চেষ্টা করুন।",
    ))
}

async fn send_chat_request(
    client: &reqwest::Client,
    api_key: &str,
    messages: &[GroqMessage],
    definitions: &[ToolDefinition],
) -> Result<reqwest::Response, reqwest::Error> {
    let payload = ChatRequest {
        model: LLM_MODEL,
        messages,
        tools: (!definitions.is_empty()).then_some(definitions),
        tool_choice: (!definitions.is_empty()).then_some("auto"),
        temperature: 0.4,
    };

    let response = client
        .post(GROQ_CHAT_URL)
        .bearer_auth(api_key)
        .json(&payload)
        .send()
        .await?;

    if response.status().as_u16() == 429
        && let Some(secs) = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<u64>().ok())
            .filter(|&s| s > 0 && s <= 2)
    {
        tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
        return client
            .post(GROQ_CHAT_URL)
            .bearer_auth(api_key)
            .json(&payload)
            .send()
            .await;
    }

    Ok(response)
}

pub(crate) fn friendly_http_error(status: u16) -> &'static str {
    match status {
        429 => "দুঃখিত স্যার, এখন অনেক রিকোয়েস্ট আসছে তাই আমি এই মুহূর্তে এই কাজটি করতে পারছি না। একটু পরে আবার চেষ্টা করুন।",
        500..=599 => "দুঃখিত স্যার, সার্ভারের সমস্যার কারণে আমি এই মুহূর্তে এই কাজটি করতে পারছি না। একটু পরে আবার চেষ্টা করুন।",
        _ => "দুঃখিত স্যার, আমি এই মুহূর্তে এই কাজটি করতে পারছি না। অনুগ্রহ করে পরে আবার চেষ্টা করুন।",
    }
}

fn sanitize_spoken(text: &str) -> String {
    let without_marks: String = text
        .chars()
        .filter(|ch| !is_emoji_or_symbol(*ch) && *ch != '*' && *ch != '`' && *ch != '#')
        .collect();
    without_marks.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_emoji_or_symbol(ch: char) -> bool {
    matches!(ch, '\u{FE0F}' | '\u{200D}' | '\u{2600}'..='\u{27BF}' | '\u{1F300}'..='\u{1FAFF}')
}
