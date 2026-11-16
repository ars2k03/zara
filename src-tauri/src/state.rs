use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use tokio::sync::{Mutex, oneshot};

use crate::audio::mic::MicStream;
use crate::gemini::stt::SttSession;

pub struct ActiveStt {
    pub mic: MicStream,
    pub session: SttSession,
}

pub struct ActiveTimer {
    pub label: String,
    pub ends_at: std::time::Instant,
    pub cancel: oneshot::Sender<()>,
}

#[derive(Default)]
pub struct AppState {
    pub stt: Mutex<Option<ActiveStt>>,
    pub tts_abort: Mutex<Option<(u64, oneshot::Sender<()>)>>,
    pub tts_generation: AtomicU64,
    pub pending_confirmations: Mutex<HashMap<String, oneshot::Sender<bool>>>,
    pub active_timers: Mutex<HashMap<String, ActiveTimer>>,
    pub next_id: AtomicU64,
}
