use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use rodio::{OutputStream, Sink, buffer::SamplesBuffer};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::oneshot;
use tokio::time::{Instant, sleep_until};

use super::live;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

const TTS_MODEL: &str = "models/gemini-3.1-flash-live-preview";
const TTS_VOICE: &str = "Zephyr";
const TTS_SAMPLE_RATE: u32 = 24_000;
const MAX_SEGMENT_CHARS: usize = 1500;
const IDLE_TIMEOUT_MS: u64 = 15_000;
const SECONDS_PER_WORD: f64 = 0.3;
const MIN_COVERAGE: f64 = 0.6;
const MAX_CONTINUATIONS: usize = 2;

enum AudioCommand {
    Chunk(Vec<i16>),
    End,
    Stop,
}

pub async fn speak(app: AppHandle, state: State<'_, AppState>, api_key: &str, text: &str) -> AppResult<()> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(AppError::from("Nothing to speak."));
    }

    let id = state.tts_generation.fetch_add(1, Ordering::Relaxed) + 1;
    let (abort_tx, mut abort_rx) = oneshot::channel::<()>();

    {
        let mut guard = state.tts_abort.lock().await;
        if let Some((_, previous)) = guard.take() {
            let _ = previous.send(());
        }
        *guard = Some((id, abort_tx));
    }

    let result = speak_stream(app, &mut abort_rx, api_key, trimmed).await;

    {
        let mut guard = state.tts_abort.lock().await;
        if let Some((current_id, _)) = *guard {
            if current_id == id {
                *guard = None;
            }
        }
    }

    result
}

pub async fn stop(state: State<'_, AppState>) -> AppResult<()> {
    let mut guard = state.tts_abort.lock().await;
    if let Some((_, abort_tx)) = guard.take() {
        let _ = abort_tx.send(());
    }
    Ok(())
}

async fn speak_stream(
    app: AppHandle,
    abort_rx: &mut oneshot::Receiver<()>,
    api_key: &str,
    text: &str,
) -> AppResult<()> {
    let (ready_tx, ready_rx) = oneshot::channel::<AppResult<()>>();
    let (audio_tx, audio_rx) = mpsc::channel::<AudioCommand>();

    let player_handle = tokio::task::spawn_blocking(move || run_audio_player(ready_tx, audio_rx));

    ready_rx
        .await
        .map_err(|_| AppError::from("Audio player failed to initialize."))??;

    let bangla = is_bengali(text);
    let segments = split_for_speech(text);

    let setup = json!({
        "model": TTS_MODEL,
        "generationConfig": {
            "responseModalities": ["AUDIO"],
            "speechConfig": {
                "voiceConfig": {
                    "prebuiltVoiceConfig": {
                        "voiceName": TTS_VOICE
                    }
                }
            }
        },
        "outputAudioTranscription": {}
    });

    let mut socket = tokio::select! {
        _ = &mut *abort_rx => {
            let _ = audio_tx.send(AudioCommand::Stop);
            return Err(AppError::from("Client cancelled the speech request."));
        }
        res = live::connect(api_key, setup) => res?,
    };

    let overall_timeout_ms = (30_000 + (text.len() as u64) * 150).min(180_000);
    let overall_deadline = Instant::now() + Duration::from_millis(overall_timeout_ms);
    let mut idle_deadline = Instant::now() + Duration::from_millis(IDLE_TIMEOUT_MS);

    let mut segment_index = 0usize;
    let mut continuations = 0usize;
    let mut turn_text = segments[0].clone();
    let mut turn_transcript = String::new();
    let mut turn_audio_bytes = 0usize;
    let mut got_audio = false;

    live::send_json(
        &mut socket,
        json!({
            "clientContent": {
                "turns": [{
                    "role": "user",
                    "parts": [{ "text": read_prompt(&turn_text, bangla) }]
                }],
                "turnComplete": true
            }
        }),
    )
    .await?;

    loop {
        let next_msg = tokio::select! {
            _ = &mut *abort_rx => {
                let _ = audio_tx.send(AudioCommand::Stop);
                let _ = socket.close(None).await;
                return Err(AppError::from("Client cancelled the speech request."));
            }
            _ = sleep_until(overall_deadline) => {
                let _ = audio_tx.send(AudioCommand::Stop);
                let _ = socket.close(None).await;
                return Err(AppError::from("Speech request timed out."));
            }
            _ = sleep_until(idle_deadline) => {
                let _ = audio_tx.send(AudioCommand::Stop);
                let _ = socket.close(None).await;
                return Err(AppError::from("Speech stopped responding."));
            }
            res = live::next_json(&mut socket) => res,
        };

        let Some(value) = next_msg? else {
            let _ = audio_tx.send(AudioCommand::Stop);
            return Err(AppError::from("Speech connection closed early."));
        };

        idle_deadline = Instant::now() + Duration::from_millis(IDLE_TIMEOUT_MS);

        if let Some(error) = value.get("error") {
            let _ = audio_tx.send(AudioCommand::Stop);
            return Err(AppError::Message(format!("Gemini speech error: {error}")));
        }

        let Some(content) = value.get("serverContent") else {
            continue;
        };

        if let Some(transcript_text) = content
            .get("outputTranscription")
            .or_else(|| content.get("output_transcription"))
            .and_then(|t| t.get("text"))
            .and_then(Value::as_str)
        {
            turn_transcript.push_str(transcript_text);
        }

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
                if usable == 0 {
                    continue;
                }

                let mut chunk_samples = Vec::with_capacity(usable / 2);
                for index in (0..usable).step_by(2) {
                    chunk_samples.push(i16::from_le_bytes([bytes[index], bytes[index + 1]]));
                }

                if !chunk_samples.is_empty() {
                    if !got_audio {
                        got_audio = true;
                        let _ = app.emit("tts://start", ());
                    }
                    turn_audio_bytes += usable;
                    let _ = audio_tx.send(AudioCommand::Chunk(chunk_samples));
                }
            }
        }

        let interrupted = content
            .get("interrupted")
            .and_then(Value::as_bool)
            .unwrap_or(false);
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

        let is_done = turn_audio_bytes > 0
            && (generation_complete || (turn_complete && !interrupted));

        if is_done {
            let audio_seconds = (turn_audio_bytes as f64) / 2.0 / (TTS_SAMPLE_RATE as f64);
            let remainder = unspoken_remainder(&turn_text, &turn_transcript, audio_seconds);

            if !remainder.is_empty() && continuations < MAX_CONTINUATIONS {
                continuations += 1;
                turn_text = remainder;
                turn_transcript.clear();
                turn_audio_bytes = 0;
                live::send_json(
                    &mut socket,
                    json!({
                        "clientContent": {
                            "turns": [{
                                "role": "user",
                                "parts": [{ "text": read_prompt(&turn_text, bangla) }]
                            }],
                            "turnComplete": true
                        }
                    }),
                )
                .await?;
                continue;
            }

            segment_index += 1;
            continuations = 0;
            if segment_index < segments.len() {
                turn_text = segments[segment_index].clone();
                turn_transcript.clear();
                turn_audio_bytes = 0;
                live::send_json(
                    &mut socket,
                    json!({
                        "clientContent": {
                            "turns": [{
                                "role": "user",
                                "parts": [{ "text": read_prompt(&turn_text, bangla) }]
                            }],
                            "turnComplete": true
                        }
                    }),
                )
                .await?;
                continue;
            }

            break;
        }
    }

    let _ = socket.close(None).await;

    if !got_audio {
        let _ = audio_tx.send(AudioCommand::Stop);
        return Err(AppError::from("No audio was generated."));
    }

    let _ = audio_tx.send(AudioCommand::End);

    tokio::select! {
        _ = &mut *abort_rx => {
            let _ = audio_tx.send(AudioCommand::Stop);
            Err(AppError::from("Client cancelled the speech request."))
        }
        res = player_handle => {
            res.map_err(|error| AppError::Message(format!("Audio player join failed: {error}")))?
        }
    }
}

fn run_audio_player(
    ready_tx: oneshot::Sender<AppResult<()>>,
    rx: Receiver<AudioCommand>,
) -> AppResult<()> {
    let (_stream, stream_handle) = match OutputStream::try_default() {
        Ok(res) => res,
        Err(error) => {
            let _ = ready_tx.send(Err(AppError::Message(format!(
                "Audio output unavailable: {error}"
            ))));
            return Ok(());
        }
    };

    let sink = match Sink::try_new(&stream_handle) {
        Ok(res) => res,
        Err(error) => {
            let _ = ready_tx.send(Err(AppError::Message(format!("Audio sink failed: {error}"))));
            return Ok(());
        }
    };

    let _ = ready_tx.send(Ok(()));

    while let Ok(command) = rx.recv() {
        match command {
            AudioCommand::Chunk(samples) => {
                sink.append(SamplesBuffer::new(1, TTS_SAMPLE_RATE, samples));
            }
            AudioCommand::End => {
                while !sink.empty() {
                    match rx.try_recv() {
                        Ok(AudioCommand::Stop) => {
                            sink.stop();
                            return Ok(());
                        }
                        _ => thread::sleep(Duration::from_millis(20)),
                    }
                }
                break;
            }
            AudioCommand::Stop => {
                sink.stop();
                break;
            }
        }
    }

    Ok(())
}

fn is_bengali(text: &str) -> bool {
    text.chars().any(|c| ('\u{0980}'..='\u{09FF}').contains(&c))
}

fn read_prompt(text: &str, bangla: bool) -> String {
    if bangla {
        format!("Repeat the following text exactly.\nOutput nothing else.\n\n{text}")
    } else {
        format!("Speak in a clear Indian English accent.\nDo not use an American accent.\nRepeat the following text exactly.\nOutput nothing else.\n\n{text}")
    }
}

fn count_words(text: &str) -> usize {
    text.split_whitespace()
        .filter(|word| word.chars().any(|c| c.is_alphanumeric()))
        .count()
}

fn count_letters(text: &str) -> usize {
    text.chars().filter(|c| c.is_alphanumeric()).count()
}

fn unspoken_remainder(requested: &str, transcript: &str, audio_seconds: f64) -> String {
    let words: Vec<&str> = requested.split_whitespace().collect();
    let total_words = count_words(requested);
    let total_letters = count_letters(requested);
    if total_words < 3 || total_letters == 0 {
        return String::new();
    }

    let spoken_letters = count_letters(transcript).min(total_letters);
    let coverage = (spoken_letters as f64 / total_letters as f64)
        .max(audio_seconds / (total_words as f64 * SECONDS_PER_WORD));

    if coverage >= MIN_COVERAGE {
        return String::new();
    }

    let mut resume_at = 0usize;
    if spoken_letters > 0 {
        let mut letters = 0usize;
        while resume_at < words.len() && letters + count_letters(words[resume_at]) <= spoken_letters {
            letters += count_letters(words[resume_at]);
            resume_at += 1;
        }
    } else {
        resume_at = (audio_seconds / (SECONDS_PER_WORD * 1.5)) as usize;
    }

    if resume_at >= words.len() {
        return String::new();
    }

    let remainder = words[resume_at..].join(" ");
    let remainder_trimmed = remainder.trim();
    if count_words(remainder_trimmed) >= 2 {
        remainder_trimmed.to_string()
    } else {
        String::new()
    }
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
