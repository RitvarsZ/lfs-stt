use std::fmt::Display;
use tokio::{sync::mpsc::{self, Receiver}, task::JoinHandle};
use tracing::info;
use whisper_rs::{FullParams, WhisperContext, WhisperContextParameters, install_logging_hooks};
use crate::{audio::{AudioPipelineError}, global::CONFIG};

pub enum SttMessageType {
    TranscriptionError,
    TranscriptionResult,
}

pub struct SttMessage {
    pub msg_type: SttMessageType,
    pub content: String,
}

impl Display for SttMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.msg_type {
            SttMessageType::TranscriptionError => write!(f, "[STT ERROR] {}", self.content),
            SttMessageType::TranscriptionResult => write!(f, "[STT TRANSCRIPTION] {}", self.content),
        }
    }
}

impl SttMessage {
    pub fn new(msg_type: SttMessageType, content: String) -> Self {
        Self { msg_type, content }
    }
}

pub async fn init(
    mut audio_in: Receiver<Vec<f32>>
) -> Result<(Receiver<SttMessage>, JoinHandle<Result<(), AudioPipelineError>>), AudioPipelineError> {
    let (event_tx, event_rx) = mpsc::channel::<SttMessage>(1);

    let handle = tokio::spawn(async move {
        install_logging_hooks();
        let mut params = WhisperContextParameters::new();
        params.use_gpu(CONFIG.use_gpu);
        // check if model path exists:
        if !std::path::Path::new(&CONFIG.model_path).exists() {
            return Err(AudioPipelineError::ModelNotFound);
        }
        let whisper_ctx = WhisperContext::new_with_params(CONFIG.model_path.as_str(), params)?;
        let mut whisper_state = match whisper_ctx.create_state() {
            Ok(state) => state,
            Err(err) => {return Err(err.into());}
        };
        let mut full_params = FullParams::new(whisper_rs::SamplingStrategy::Greedy { best_of: 8 });
        full_params.set_language(Some("en"));
        full_params.set_print_special(false);
        full_params.set_print_progress(false);
        full_params.set_print_realtime(false);
        full_params.set_print_timestamps(false);

        info!("✅ STT thread started");

        loop {
            while let Some(audio_buffer) = audio_in.recv().await {
                match maybe_dump_buffer_to_wav(&audio_buffer) {
                    Ok(_) => (),
                    Err(err) => { return Err(err); }
                };
                if let Err(err) = whisper_state.full(full_params.clone(), &audio_buffer) {
                    let _ = event_tx.send(
                        SttMessage::new(
                            SttMessageType::TranscriptionError,
                            format!("❌ Transcription error: {:?}", err)
                        )
                    ).await;
                    continue;
                }

                let mut text = String::new();
                let n_segments = whisper_state.full_n_segments();
                for i in 0..n_segments {
                    if let Some(segment) = whisper_state.get_segment(i) && let Ok(segment) = segment.to_str() {
                        text.push_str(segment);
                    }
                }

                let _ = event_tx.send(
                    SttMessage::new(
                        SttMessageType::TranscriptionResult,
                        text.trim().to_string()
                    )
                ).await;
            }
        }
    });

    Ok((event_rx, handle))
}

fn maybe_dump_buffer_to_wav(samples: &[f32]) -> Result<(), AudioPipelineError> {
    if !CONFIG.debug_audio_resampling { return Ok(()); }

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create("debug.wav", spec)
        .map_err(|e| AudioPipelineError::AudioDebugError(format!("Failed to create WAV writer: {}", e)))?;
    for &sample in samples {
        writer.write_sample(sample)
            .map_err(|e| AudioPipelineError::AudioDebugError(format!("Failed to write debug audio into file: {}", e)))?;
    }
    writer.finalize()
        .map_err(|e| AudioPipelineError::AudioDebugError(format!("Failed to finalize WAV file: {}", e)))?;

    Ok(())
}
