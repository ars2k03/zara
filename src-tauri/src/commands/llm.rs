use crate::error::AppResult;
use crate::providers::groq::{self, ChatMessage};

pub async fn reply(messages: Vec<ChatMessage>) -> AppResult<String> {
    groq::reply(messages).await
}
