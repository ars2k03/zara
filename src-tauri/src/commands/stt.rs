use tauri::{AppHandle, Emitter, State};
use tokio::sync::mpsc;

use crate::audio::mic::{CAPTURE_SAMPLE_RATE, MicStream};
use crate::config;
use crate::error::{AppError, AppResult};
use crate::gemini::stt::{SttEvents, SttSession};
use crate::state::{ActiveStt, AppState};

pub async fn start(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let mut slot = state.stt.lock().await;
    *slot = None;

    let api_key = config::require("GOOGLE_API_KEY")?;
    let (interim_tx, mut interim_rx) = mpsc::unbounded_channel::<String>();
    let (final_tx, mut final_rx) = mpsc::unbounded_channel::<String>();

    let app_interim = app.clone();
    tokio::spawn(async move {
        while let Some(text) = interim_rx.recv().await {
            let _ = app_interim.emit("stt://interim", text);
        }
    });

    let app_final = app.clone();
    tokio::spawn(async move {
        while let Some(text) = final_rx.recv().await {
            let _ = app_final.emit("stt://final", text);
        }
    });

    let session = SttSession::start(api_key, CAPTURE_SAMPLE_RATE, SttEvents { interim: interim_tx, final_text: final_tx }).await?;
    let mic = MicStream::start(app, session.handle())?;
    *slot = Some(ActiveStt { mic, session });

    Ok(())
}

pub async fn stop(state: State<'_, AppState>) -> AppResult<String> {
    let mut slot = state.stt.lock().await;
    let active = slot.take().ok_or_else(|| AppError::from("STT is not running."))?;
    let ActiveStt { mic, session } = active;
    drop(mic);
    session.stop().await
}
