use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SupportedStreamConfig};
use tauri::{AppHandle, Emitter};

use crate::error::{AppError, AppResult};
use crate::gemini::stt::SttHandle;

pub const CAPTURE_SAMPLE_RATE: u32 = 16_000;

// cpal::Stream is intentionally !Send, so it lives on a dedicated thread for
// its whole lifetime; this guard only owns the stop signal and the thread.
pub struct MicStream {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl MicStream {
    pub fn start(app: AppHandle, stt: SttHandle) -> AppResult<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel::<AppResult<()>>();

        let thread_stop = Arc::clone(&stop);
        let thread = std::thread::spawn(move || capture(app, stt, thread_stop, ready_tx));

        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Self {
                stop,
                thread: Some(thread),
            }),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(AppError::from("Microphone thread did not start.")),
        }
    }
}

impl Drop for MicStream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn capture(app: AppHandle, stt: SttHandle, stop: Arc<AtomicBool>, ready: mpsc::Sender<AppResult<()>>) {
    let stream = match build_stream(app, stt) {
        Ok(stream) => stream,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    if let Err(error) = stream.play() {
        let _ = ready.send(Err(AppError::Message(format!("Microphone start failed: {error}"))));
        return;
    }
    let _ = ready.send(Ok(()));

    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn build_stream(app: AppHandle, stt: SttHandle) -> AppResult<cpal::Stream> {
    let device = cpal::default_host()
        .default_input_device()
        .ok_or_else(|| AppError::from("No microphone device found."))?;
    let config = device
        .default_input_config()
        .map_err(|error| AppError::Message(format!("Microphone config failed: {error}")))?;

    match config.sample_format() {
        SampleFormat::F32 => connect::<f32>(&device, &config, |sample| sample, app, stt),
        SampleFormat::I16 => connect::<i16>(&device, &config, |sample| sample as f32 / i16::MAX as f32, app, stt),
        SampleFormat::U16 => connect::<u16>(&device, &config, |sample| {
            (f32::from(sample) - f32::from(u16::MAX) / 2.0) / (f32::from(u16::MAX) / 2.0)
        }, app, stt),
        format => Err(AppError::Message(format!("Unsupported microphone format: {format}"))),
    }
}

fn connect<T: cpal::SizedSample + Send + 'static>(
    device: &cpal::Device,
    config: &SupportedStreamConfig,
    to_f32: fn(T) -> f32,
    app: AppHandle,
    stt: SttHandle,
) -> AppResult<cpal::Stream> {
    let channels = usize::from(config.channels());
    let input_rate = config.sample_rate().0;

    device
        .build_input_stream(
            &config.clone().into(),
            move |samples: &[T], _: &cpal::InputCallbackInfo| {
                let mono: Vec<f32> = samples
                    .chunks(channels)
                    .filter_map(|frame| frame.first().map(|&sample| to_f32(sample)))
                    .collect();
                let resampled = resample(&mono, input_rate, CAPTURE_SAMPLE_RATE);
                let _ = app.emit("stt://level", rms(&resampled));
                stt.push(to_pcm16(&resampled));
            },
            |error| eprintln!("microphone stream error: {error}"),
            None,
        )
        .map_err(|error| AppError::Message(format!("Microphone stream failed: {error}")))
}

fn resample(input: &[f32], input_rate: u32, output_rate: u32) -> Vec<f32> {
    if input_rate == output_rate {
        return input.to_vec();
    }
    let ratio = f64::from(input_rate) / f64::from(output_rate);
    let length = (input.len() as f64 / ratio).max(1.0) as usize;
    let mut output = Vec::with_capacity(length);
    for index in 0..length {
        let position = index as f64 * ratio;
        let base = position.floor() as usize;
        let fraction = (position - position.floor()) as f32;
        let current = input.get(base).copied().unwrap_or(0.0);
        let next = input.get(base + 1).copied().unwrap_or(current);
        output.push(current + (next - current) * fraction);
    }
    output
}

fn to_pcm16(samples: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32).sqrt()
}
