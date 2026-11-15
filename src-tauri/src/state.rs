use std::sync::atomic::AtomicU64;
use tokio::sync::{Mutex, oneshot};

use crate::audio::mic::MicStream;
use crate::gemini::stt::SttSession;

pub struct ActiveStt {
    pub mic: MicStream,
    pub session: SttSession,
}

#[derive(Default)]
pub struct AppState {
    pub stt: Mutex<Option<ActiveStt>>,
    pub tts_abort: Mutex<Option<(u64, oneshot::Sender<()>)>>,
    pub tts_generation: AtomicU64,
}
