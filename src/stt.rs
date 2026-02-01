use std::{fmt::Display, sync::{Arc, Mutex, mpsc::{self, Receiver}}, thread::{self}};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::{DEBUG_AUDIO_RESAMPLING, RECORDING_TIMEOUT_SECS};
use crate::USE_GPU;
use crate::MODEL_PATH;

pub enum SttThreadMessageType {
    Log,
    TranscriptionError,
    TranscriptionResult,
    RecordingTimeoutReached,
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
            SttThreadMessageType::RecordingTimeoutReached => write!(f, "[STT TIMEOUT REACHED] {}", self.content),
        }
    }
}

impl SttThreadMessage {
    pub fn new(msg_type: SttThreadMessageType, content: String) -> Self {
        Self { msg_type, content }
    }
}

pub struct SttContext {
    pub is_recording: Arc<Mutex<bool>>,
    pub log_rx: mpsc::Receiver<SttThreadMessage>,
    log_tx: mpsc::Sender<SttThreadMessage>,
}

impl SttContext {
    pub fn new() -> Self {
        let is_recording = Arc::new(Mutex::new(false));
        let (log_tx, log_rx) = mpsc::channel::<SttThreadMessage>(); // logs + transcription to main thread

        Self {
            is_recording,
            log_tx,
            log_rx,
        }
    }
}

pub fn start_stt_worker(
    ctx: &SttContext,
    audio_in: Receiver<Vec<f32>>
) {
    let log_tx = ctx.log_tx.clone();
    let is_recording = ctx.is_recording.clone();

    thread::spawn(move || {
        let mut params = WhisperContextParameters::new();
        params.use_gpu(USE_GPU);
        let whisper_ctx = WhisperContext::new_with_params(MODEL_PATH, params)
                .expect("Failed to create Whisper context");
        let mut whisper_state = whisper_ctx.create_state().unwrap();
        let mut audio_buffer = Vec::<f32>::new();
        let mut full_params = FullParams::new(SamplingStrategy::Greedy { best_of: 8 });
        full_params.set_language(Some("en"));
        full_params.set_print_special(false);
        full_params.set_print_progress(false);
        full_params.set_print_realtime(false);
        full_params.set_print_timestamps(false);

        let _ = log_tx.send(
            SttThreadMessage::new(
                SttThreadMessageType::Log,
                "✅ STT thread started".into()
            )
        );


        loop {
            // While recording, dump samples into buffer.
            if *is_recording.lock().unwrap() {
                while let Ok(samples) = audio_in.try_recv() {
                    audio_buffer.extend_from_slice(&samples);
                }

                // if audio goes over configured timeout seconds, stop recording and process.
                // this is in place in case we start recording and forget
                // or recording was started accidentally.
                if audio_buffer.len() >= 16_000 * RECORDING_TIMEOUT_SECS as usize {
                    *is_recording.lock().unwrap() = false;
                    let _ = log_tx.send(
                        SttThreadMessage::new(
                            SttThreadMessageType::RecordingTimeoutReached,
                            String::from(""),
                        )
                    );
                } else {
                    continue;
                }
            }

            // When not recording and have samples, transcribe and clear the buffer.
            if audio_buffer.is_empty() {
                continue;
            }

            let _ = maybe_dump_buffer_to_wav(&audio_buffer);
            if let Err(err) = whisper_state.full(full_params.clone(), &audio_buffer) {
                let _ = log_tx.send(
                    SttThreadMessage::new(
                        SttThreadMessageType::TranscriptionError,
                        format!("❌ Transcription error: {:?}", err)
                    )
                );
                continue;
            }

            let mut text = String::new();
            let n_segments = whisper_state.full_n_segments();
            for i in 0..n_segments {
                if let Some(segment) = whisper_state.get_segment(i) && let Ok(segment) = segment.to_str() {
                    text.push_str(segment);
                }
            }

            let _ = log_tx.send(
                SttThreadMessage::new(
                    SttThreadMessageType::TranscriptionResult,
                    text.trim().to_string()
                )
            );

            audio_buffer.clear();
        }
    });
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

