use serde::{Deserialize, Serialize};

use crate::config;
use crate::error::{AppError, AppResult};

const GROQ_CHAT_URL: &str = "https://api.groq.com/openai/v1/chat/completions";
const LLM_MODEL: &str = "openai/gpt-oss-120b";

const SYSTEM_PROMPT: &str = "\
You are Zara, a helpful voice assistant.

Language:
- Reply mainly in Bangla, written in Bangla script.
- Use natural Bangla–English code-switching: keep words in English when they sound clearer or more natural that way (e.g. AI, API, app, file, okay, wifi, laptop).
- Do not write full answers only in English.
- Do not write Bangla in Latin letters.

Style:
- Keep replies short and conversational — they will be spoken aloud.
- No emoji, emoticons, or markdown.
- Prefer plain spoken sentences over lists or headings.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Serialize)]
struct ChatRequest {
    model: &'static str,
    messages: Vec<ChatMessage>,
    temperature: f32,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChatMessage,
}

pub async fn reply(history: Vec<ChatMessage>) -> AppResult<String> {
    let api_key = config::require("GROQ_API_KEY")?;
    let mut messages = vec![ChatMessage {
        role: "system".into(),
        content: SYSTEM_PROMPT.into(),
    }];
    messages.extend(history);

    let client = reqwest::Client::new();
    let response = client
        .post(GROQ_CHAT_URL)
        .bearer_auth(api_key)
        .json(&ChatRequest {
            model: LLM_MODEL,
            messages,
            temperature: 0.7,
        })
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::Message(format!("Groq LLM failed ({status}): {body}")));
    }

    let payload: ChatResponse = response.json().await?;
    let content = payload
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content)
        .map(|text| sanitize_spoken(&text))
        .filter(|text| !text.is_empty())
        .ok_or_else(|| AppError::from("Groq returned an empty reply."))?;

    Ok(content)
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
