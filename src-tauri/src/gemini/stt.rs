use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::future;

use tokio::sync::{mpsc, oneshot};
use tokio::time::{Duration, Instant, sleep_until, timeout};
use tokio_tungstenite::tungstenite::Message;

use super::live::{self, LiveSocket};
use crate::error::{AppError, AppResult};

const STT_MODEL: &str = "models/gemini-3.5-transcribe-live";
const AUDIO_QUEUE_CAPACITY: usize = 512;
const FLUSH_TIMEOUT: Duration = Duration::from_secs(2);

pub struct SttSession {
    audio_tx: mpsc::Sender<SttCommand>,
}

enum SttCommand {
    Audio(Vec<u8>),
    Stop(oneshot::Sender<AppResult<String>>),
}

#[derive(Clone)]
pub struct SttEvents {
    pub interim: mpsc::UnboundedSender<String>,
    pub final_text: mpsc::UnboundedSender<String>,
}

#[derive(Clone)]
pub struct SttHandle {
    audio_tx: mpsc::Sender<SttCommand>,
}

impl SttHandle {
    pub fn push(&self, pcm: Vec<u8>) {
        if pcm.is_empty() {
            return;
        }
        let _ = self.audio_tx.try_send(SttCommand::Audio(pcm));
    }
}

impl SttSession {
    pub async fn start(api_key: String, sample_rate: u32, events: SttEvents) -> AppResult<Self> {
        let setup = json!({
            "model": STT_MODEL,
            "generationConfig": {
                "responseModalities": ["TEXT"]
            },
            "inputAudioTranscription": {
                "languageCodes": ["bn-BD", "en-US"],
                "mode": "VERBATIM"
            }
        });

        let socket = live::connect(&api_key, setup).await?;
        let (sink, stream) = socket.split();
        let (audio_tx, audio_rx) = mpsc::channel::<SttCommand>(AUDIO_QUEUE_CAPACITY);

        tokio::spawn(run_session(sink, stream, audio_rx, events, sample_rate));

        Ok(Self { audio_tx })
    }

    pub fn handle(&self) -> SttHandle {
        SttHandle {
            audio_tx: self.audio_tx.clone(),
        }
    }

    pub async fn stop(self) -> AppResult<String> {
        let (tx, rx) = oneshot::channel();
        self.audio_tx
            .send(SttCommand::Stop(tx))
            .await
            .map_err(|_| AppError::from("STT session is closed."))?;
        match timeout(FLUSH_TIMEOUT + Duration::from_millis(500), rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(AppError::from("STT stop failed.")),
            Err(_) => Err(AppError::from("STT stop timed out.")),
        }
    }
}

async fn run_session(
    mut sink: SplitSink<LiveSocket, Message>,
    mut stream: SplitStream<LiveSocket>,
    mut audio_rx: mpsc::Receiver<SttCommand>,
    events: SttEvents,
    sample_rate: u32,
) {
    let mut finals = String::new();
    let mut interim = String::new();
    let mut stop_tx: Option<oneshot::Sender<AppResult<String>>> = None;
    let mut flush_deadline: Option<Instant> = None;

    loop {
        tokio::select! {
            command = audio_rx.recv() => {
                match command {
                    Some(SttCommand::Audio(pcm)) => {
                        if stop_tx.is_some() {
                            continue;
                        }
                        let payload = json!({
                            "realtimeInput": {
                                "audio": {
                                    "data": BASE64.encode(pcm),
                                    "mimeType": format!("audio/pcm;rate={sample_rate}")
                                }
                            }
                        });
                        if sink.send(Message::Text(payload.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Some(SttCommand::Stop(reply)) => {
                        stop_tx = Some(reply);
                        flush_deadline = Some(Instant::now() + FLUSH_TIMEOUT);
                        let payload = json!({ "realtimeInput": { "audioStreamEnd": true } });
                        let _ = sink.send(Message::Text(payload.to_string().into())).await;
                    }
                    None => break,
                }
            }
            message = stream.next() => {
                let Some(Ok(message)) = message else { break; };
                match live::message_json(message) {
                    Ok(live::WsJson::Closed) => break,
                    Ok(live::WsJson::Ignored) => continue,
                    Ok(live::WsJson::Value(value)) => {
                        if let Some(error) = value.get("error") {
                            if let Some(reply) = stop_tx.take() {
                                let _ = reply.send(Err(AppError::Message(format!("STT error: {error}"))));
                            }
                            break;
                        }
                        let Some(content) = value.get("serverContent") else { continue; };

                        if let Some(text) = transcription_text(content, "interimInputTranscription", "interim_input_transcription") {
                            interim = text.clone();
                            let _ = events.interim.send(text);
                        }

                        if let Some(text) = transcription_text(content, "inputTranscription", "input_transcription") {
                            let trimmed = text.trim();
                            if !trimmed.is_empty() {
                                if !finals.is_empty() {
                                    finals.push(' ');
                                }
                                finals.push_str(trimmed);
                                interim.clear();
                                let _ = events.final_text.send(trimmed.to_string());
                            }
                        }

                        let turn_done = content.get("turnComplete").or_else(|| content.get("turn_complete")).and_then(Value::as_bool).unwrap_or(false)
                            || content.get("interrupted").and_then(Value::as_bool).unwrap_or(false);
                        if turn_done && stop_tx.is_some() {
                            break;
                        }
                    }
                    Err(_) => continue,
                }
            }
            _ = async {
                match flush_deadline {
                    Some(deadline) => sleep_until(deadline).await,
                    None => future::pending::<()>().await,
                }
            } => {
                break;
            }
        }
    }

    let _ = sink.close().await;

    let transcript = if !finals.trim().is_empty() {
        finals.trim().to_string()
    } else {
        interim.trim().to_string()
    };

    if let Some(reply) = stop_tx.take() {
        let _ = reply.send(Ok(transcript));
    }
}

fn transcription_text(content: &Value, camel: &str, snake: &str) -> Option<String> {
    content
        .pointer(&format!("/{camel}/text"))
        .or_else(|| content.pointer(&format!("/{snake}/text")))
        .and_then(Value::as_str)
        .map(str::to_string)
}
