use tokio::sync::Mutex;

use crate::audio::mic::MicStream;
use crate::gemini::stt::SttSession;

pub struct ActiveStt {
    pub mic: MicStream,
    pub session: SttSession,
}

#[derive(Default)]
pub struct AppState {
    pub stt: Mutex<Option<ActiveStt>>,
}
