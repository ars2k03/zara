use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use rodio::{OutputStream, Sink, buffer::SamplesBuffer};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter};

use super::live;
use crate::error::{AppError, AppResult};

const TTS_MODEL: &str = "models/gemini-3.1-flash-live-preview";
const TTS_VOICE: &str = "Aoede";
const TTS_SAMPLE_RATE: u32 = 24_000;
const MAX_SEGMENT_CHARS: usize = 1500;

pub async fn speak(app: AppHandle, api_key: &str, text: &str) -> AppResult<()> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(AppError::from("Nothing to speak."));
    }

    let segments = split_for_speech(trimmed);
    let setup = json!({
        "model": TTS_MODEL,
        "generationConfig": {
            "responseModalities": ["AUDIO"],
            "speechConfig": {
                "languageCode": "bn-BD",
                "voiceConfig": {
                    "prebuiltVoiceConfig": {
                        "voiceName": TTS_VOICE
                    }
                }
            }
        },
        "outputAudioTranscription": {}
    });

    let mut socket = live::connect(api_key, setup).await?;

    let mut samples = Vec::<i16>::new();

    for segment in segments {
        live::send_json(
            &mut socket,
            json!({
                "clientContent": {
                    "turns": [{
                        "role": "user",
                        "parts": [{ "text": read_prompt(&segment) }]
                    }],
                    "turnComplete": true
                }
            }),
        )
        .await?;

        loop {
            let Some(value) = live::next_json(&mut socket).await? else {
                return Err(AppError::from("Speech connection closed early."));
            };

            if let Some(error) = value.get("error") {
                return Err(AppError::Message(format!("Gemini speech error: {error}")));
            }

            let Some(content) = value.get("serverContent") else {
                continue;
            };

            if let Some(parts) = content.pointer("/modelTurn/parts").and_then(Value::as_array) {
                for part in parts {
                    let Some(data) = part
                        .pointer("/inlineData/data")
                        .or_else(|| part.pointer("/inline_data/data"))
                        .and_then(Value::as_str)
                    else {
                        continue;
                    };
                    let mime = part
                        .pointer("/inlineData/mimeType")
                        .or_else(|| part.pointer("/inline_data/mime_type"))
                        .and_then(Value::as_str)
                        .unwrap_or("audio/pcm");
                    if !mime.starts_with("audio/pcm") && !mime.is_empty() {
                        continue;
                    }

                    let Ok(bytes) = BASE64.decode(data) else {
                        continue;
                    };
                    let usable = bytes.len() - (bytes.len() % 2);
                    for index in (0..usable).step_by(2) {
                        samples.push(i16::from_le_bytes([bytes[index], bytes[index + 1]]));
                    }
                }
            }

            let generation_complete = content
                .get("generationComplete")
                .or_else(|| content.get("generation_complete"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let turn_complete = content
                .get("turnComplete")
                .or_else(|| content.get("turn_complete"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if generation_complete || turn_complete {
                break;
            }
        }
    }

    let _ = socket.close(None).await;

    if samples.is_empty() {
        return Err(AppError::from("No audio was generated."));
    }

    tokio::task::spawn_blocking(move || play_pcm(app, samples))
        .await
        .map_err(|error| AppError::Message(format!("Audio playback join failed: {error}")))?
}

fn play_pcm(app: AppHandle, samples: Vec<i16>) -> AppResult<()> {
    let (_stream, stream_handle) = OutputStream::try_default()
        .map_err(|error| AppError::Message(format!("Audio output unavailable: {error}")))?;
    let sink = Sink::try_new(&stream_handle)
        .map_err(|error| AppError::Message(format!("Audio sink failed: {error}")))?;
    sink.append(SamplesBuffer::new(1, TTS_SAMPLE_RATE, samples));
    let _ = app.emit("tts://start", ());
    sink.sleep_until_end();
    Ok(())
}

fn read_prompt(text: &str) -> String {
    ["Repeat the following text exactly.", "Output nothing else.", "", text].join("\n")
}

fn split_for_speech(text: &str) -> Vec<String> {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let sentences = split_sentences(&normalized);
    let mut segments = Vec::new();
    let mut current = String::new();

    for sentence in sentences {
        if sentence.len() > MAX_SEGMENT_CHARS {
            if !current.is_empty() {
                segments.push(std::mem::take(&mut current));
            }
            let mut rest = sentence;
            while rest.len() > MAX_SEGMENT_CHARS {
                let window = &rest[..MAX_SEGMENT_CHARS];
                let cut = window
                    .rfind(", ")
                    .or_else(|| window.rfind(' '))
                    .filter(|&index| index > MAX_SEGMENT_CHARS / 2)
                    .map(|index| index + 1)
                    .unwrap_or(MAX_SEGMENT_CHARS);
                segments.push(rest[..cut].trim().to_string());
                rest = rest[cut..].trim().to_string();
            }
            current = rest;
            continue;
        }

        if !current.is_empty() && current.len() + 1 + sentence.len() > MAX_SEGMENT_CHARS {
            segments.push(std::mem::take(&mut current));
            current = sentence;
        } else if current.is_empty() {
            current = sentence;
        } else {
            current.push(' ');
            current.push_str(&sentence);
        }
    }

    if !current.trim().is_empty() {
        segments.push(current.trim().to_string());
    }

    if segments.is_empty() {
        vec![normalized]
    } else {
        segments
    }
}

fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut start = 0usize;
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;

    while i < chars.len() {
        let ch = chars[i];
        if matches!(ch, '.' | '!' | '?' | '।') {
            let mut end = i + 1;
            while end < chars.len() && chars[end].is_whitespace() {
                end += 1;
            }
            let sentence: String = chars[start..i + 1].iter().collect();
            let trimmed = sentence.trim();
            if !trimmed.is_empty() {
                sentences.push(trimmed.to_string());
            }
            start = end;
            i = end;
            continue;
        }
        i += 1;
    }

    if start < chars.len() {
        let sentence: String = chars[start..].iter().collect();
        let trimmed = sentence.trim();
        if !trimmed.is_empty() {
            sentences.push(trimmed.to_string());
        }
    }

    sentences
}
