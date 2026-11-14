use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

use crate::error::{AppError, AppResult};

const LIVE_WS_BASE: &str =
    "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";

pub type LiveSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub async fn connect(api_key: &str, setup: Value) -> AppResult<LiveSocket> {
    let url = format!("{LIVE_WS_BASE}?key={}", urlencoding::encode(api_key));
    let request = url.into_client_request().map_err(|error| AppError::Message(error.to_string()))?;
    let (mut socket, _) = connect_async(request)
        .await
        .map_err(|error| AppError::Message(format!("Gemini Live connect failed: {error}")))?;

    socket
        .send(Message::Text(json!({ "setup": setup }).to_string().into()))
        .await
        .map_err(|error| AppError::Message(format!("Gemini setup send failed: {error}")))?;

    loop {
        let value = next_json(&mut socket)
            .await?
            .ok_or_else(|| AppError::from("Gemini Live closed before setup completed."))?;

        if value.get("setupComplete").is_some() {
            return Ok(socket);
        }
        if let Some(error) = value.get("error") {
            return Err(AppError::Message(format!("Gemini setup error: {error}")));
        }
    }
}

pub async fn send_json(socket: &mut LiveSocket, payload: Value) -> AppResult<()> {
    socket
        .send(Message::Text(payload.to_string().into()))
        .await
        .map_err(|error| AppError::Message(format!("Gemini send failed: {error}")))
}

#[derive(Debug)]
pub enum WsJson {
    Value(Value),
    Closed,
    Ignored,
}

// Server sends every message (setupComplete, transcription, audio) as binary
// frames whose payload is UTF-8 JSON — text frames almost never arrive.
pub fn message_json(message: Message) -> AppResult<WsJson> {
    match message {
        Message::Text(text) => Ok(WsJson::Value(serde_json::from_str(&text)?)),
        Message::Binary(data) => Ok(WsJson::Value(serde_json::from_slice(&data)?)),
        Message::Close(_) => Ok(WsJson::Closed),
        _ => Ok(WsJson::Ignored),
    }
}

pub async fn next_json(socket: &mut LiveSocket) -> AppResult<Option<Value>> {
    while let Some(message) = socket.next().await {
        let message = message.map_err(|error| AppError::Message(format!("Gemini read failed: {error}")))?;
        match message_json(message)? {
            WsJson::Value(value) => return Ok(Some(value)),
            WsJson::Closed => return Ok(None),
            WsJson::Ignored => {}
        }
    }
    Ok(None)
}
