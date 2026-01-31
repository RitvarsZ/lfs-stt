use std::{fmt::Display, sync::{Arc, Mutex, mpsc}, thread};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::DEBUG_AUDIO_RESAMPLING;
use crate::USE_GPU;
use crate::MODEL_PATH;

pub enum SttThreadMessageType {
    Log,
    TranscriptionError,
    TranscriptionResult,
}

pub struct SttThreadMessage {
    pub msg_type: SttThreadMessageType,
    pub content: String,
}

impl Display for SttThreadMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.msg_type {
            SttThreadMessageType::Log => write!(f, "[STT LOG] {}", self.content),
            SttThreadMessageType::TranscriptionError => write!(f, "[STT ERROR] {}", self.content),
            SttThreadMessageType::TranscriptionResult => write!(f, "[STT TRANSCRIPTION] {}", self.content),
        }
    }
}

impl SttThreadMessage {
    pub fn new(msg_type: SttThreadMessageType, content: String) -> Self {
        Self { msg_type, content }
    }
}

pub struct SttContext {
    pub audio_data: Arc<Mutex<Vec<f32>>>,
    pub recording: Arc<Mutex<bool>>,
    pub audio_tx: mpsc::Sender<Vec<f32>>,
    pub log_rx: mpsc::Receiver<SttThreadMessage>,
}

impl SttContext {
    fn new(
        audio_data: Arc<Mutex<Vec<f32>>>,
        recording: Arc<Mutex<bool>>,
        audio_tx: mpsc::Sender<Vec<f32>>,
        log_rx: mpsc::Receiver<SttThreadMessage>,
    ) -> Self {
        Self {
            audio_data,
            recording,
            audio_tx,
            log_rx,
        }
    }

    pub fn init_stt() -> Self {
        // 1️⃣ Setup Whisper
        let mut params = WhisperContextParameters::new();
        params.use_gpu(USE_GPU);
        let ctx = Arc::new(
            WhisperContext::new_with_params(MODEL_PATH, params)
                .expect("Failed to create Whisper context")
        );

        // 2️⃣ Shared audio buffer and recording flag
        let audio_data = Arc::new(Mutex::new(Vec::<f32>::new()));
        let recording = Arc::new(Mutex::new(false));

        // 3️⃣ Channels
        let (audio_tx, audio_rx) = mpsc::channel::<Vec<f32>>(); // audio buffers to STT thread
        let (log_tx, log_rx) = mpsc::channel::<SttThreadMessage>(); // logs + transcription to main thread

        // STT thread
        let ctx_clone = ctx.clone();
        thread::spawn(move || {
            let mut state = ctx_clone.create_state().unwrap();
            let _ = log_tx.send(
                SttThreadMessage::new(
                    SttThreadMessageType::Log,
                    "✅ STT thread started".into()
                )
            );

            let mut full_params = FullParams::new(SamplingStrategy::Greedy { best_of: 8 });
            full_params.set_language(Some("en"));
            full_params.set_print_special(false);
            full_params.set_print_progress(false);
            full_params.set_print_realtime(false);
            full_params.set_print_timestamps(false);

            while let Ok(samples) = audio_rx.recv() {
                // log_tx.send(format!("📦 Received {} samples for transcription", samples.len())).unwrap();
                let _ = log_tx.send(
                    SttThreadMessage::new(
                        SttThreadMessageType::Log,
                        format!("📦 Received {} samples for transcription", samples.len())
                    )
                );

                if let Err(err) = state.full(full_params.clone(), &samples) {
                    let _ = log_tx.send(
                        SttThreadMessage::new(
                            SttThreadMessageType::TranscriptionError,
                            format!("❌ Transcription error: {:?}", err)
                        )
                    );
                    continue;
                }

                let mut text = String::new();
                let n_segments = state.full_n_segments();
                for i in 0..n_segments {
                    if let Some(segment) = state.get_segment(i) && let Ok(segment) = segment.to_str() {
                        text.push_str(segment);
                    }
                }

                let _ = log_tx.send(
                    SttThreadMessage::new(
                        SttThreadMessageType::TranscriptionResult,
                        text.trim().to_string()
                    )
                );
            }
        });
        Self::new(audio_data, recording, audio_tx, log_rx)
    }
}

pub fn maybe_dump_buffer_to_wav(samples: &[f32]) -> Result<(), Box<dyn std::error::Error>> {
    if !DEBUG_AUDIO_RESAMPLING { return Ok(()); }

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create("debug.wav", spec)?;
    for &sample in samples {
        writer.write_sample(sample)?;
    }
    writer.finalize()?;
    Ok(())
}

