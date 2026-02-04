use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use cpal::Stream;
use tokio::{sync::{mpsc::{self, Receiver}}, task::JoinHandle};
use tracing::{debug, error, info};
use crate::{RECORDING_TIMEOUT_SECS, audio::{self, speech_to_text::SttMessage}};

pub enum CaptureMsg {
    Audio(Vec<f32>),
    Stop,
    Error(String),
}

pub struct AudioPipeline {
    is_recording: Arc<AtomicBool>,
    resampled_tx: mpsc::Sender<CaptureMsg>,
    _stream: Stream, // Keep alive
}

impl AudioPipeline {
    pub async fn new() -> Result<(Self, Receiver<SttMessage>, JoinHandle<()>), Box<dyn std::error::Error>> {
        let is_recording = Arc::new(AtomicBool::new(false));
        let (stt_tx, audio_buffer_rx) = mpsc::channel::<Vec<f32>>(1);

        let (stream, stream_config, recorder_rx) = audio::recorder::init(is_recording.clone())?;
        let (resampled_tx, resampled_rx, resampler_handle) = audio::resampler::init(
            recorder_rx,
            stream_config.sample_rate as usize,
            stream_config.input_channels,
        ).await?;
        let capture_handle = init_audio_capture(
            resampled_rx,
            stt_tx,
            is_recording.clone(),
        ).await?;
        let (stt_rx, stt_handle) = audio::speech_to_text::init(audio_buffer_rx).await?;

        let handle = watch_audio_handles(vec![
            resampler_handle,
            capture_handle,
            stt_handle,
        ]).await;

        let pipeline = AudioPipeline {
            is_recording,
            resampled_tx,
            _stream: stream,
        };

        Ok((pipeline, stt_rx, handle))
    }

    /// Start stream and accumulate resampled audio into buffer.
    /// If buffer reaches timeout size, stop recording and transcribe.
    pub async fn start_recording(&self) {
        self.is_recording.store(true, Ordering::Relaxed);
    }

    /// Stop stream, send accumulated audio_buffer to STT, and clear buffer.
    pub async fn stop_recording_and_transcribe(&self) {
        self.is_recording.store(false, Ordering::Relaxed);
        let _ = self.resampled_tx.send(CaptureMsg::Stop).await;
    }
}

async fn init_audio_capture(
    mut rx: mpsc::Receiver<CaptureMsg>,
    tx: mpsc::Sender<Vec<f32>>,
    is_recording: Arc<AtomicBool>,
) -> Result<JoinHandle<()>, Box<dyn std::error::Error>> {
    let handle = tokio::spawn(async move {
        let mut buffer = Vec::<f32>::with_capacity(16_000 * RECORDING_TIMEOUT_SECS as usize);

        debug!("Audio capture task started, waiting for audio data...");
        loop {
            if let Some(data) = rx.recv().await {
                match data {
                    CaptureMsg::Error(err) => {
                        error!(err);
                        break;
                    },
                    CaptureMsg::Stop => {
                        if !buffer.is_empty() {
                            if tx.send(buffer.clone()).await.is_err() {
                                break;
                            }
                            buffer.clear();
                        }
                    },
                    CaptureMsg::Audio(data) => {
                        buffer.extend_from_slice(&data);
                        if buffer.len() >= 16_000 * RECORDING_TIMEOUT_SECS as usize {
                            debug!("Buffer reached timeout size, sending to STT");
                            is_recording.store(false, Ordering::Relaxed);
                            if tx.send(buffer.clone()).await.is_err() {
                                break;
                            }
                            buffer.clear();
                        }
                    }
                }
            }
        }
    });

    Ok(handle)
}

async fn watch_audio_handles(handles: Vec<JoinHandle<()>>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let (completed, _index, remaining) = futures::future::select_all(handles).await;

        if let Err(e) = completed {
            error!("One of the audio pipeline tasks panicked: {:?}", e);
        } else {
            info!("One of the audio pipeline tasks finished");
        }

        info!("Aborting remaining audio pipeline tasks...");
        for handle in remaining {
            handle.abort();
        }
    })
}

