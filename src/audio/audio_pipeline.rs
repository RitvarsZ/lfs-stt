use std::sync::Arc;

use cpal::traits::StreamTrait;
use tokio::{sync::{Mutex, mpsc::{self, Receiver}}, task::JoinHandle};

use crate::{RECORDING_TIMEOUT_SECS, audio::{self, speech_to_text::SttMessage}};

pub struct AudioPipeline {
    stream: cpal::Stream,
    is_recording: Arc<Mutex<bool>>,
    resampler_handle: JoinHandle<()>,
    stt_handle: JoinHandle<()>,
    capture_handle: JoinHandle<()>,
}

impl AudioPipeline {
    pub async fn new() -> Result<(Self, Receiver<SttMessage>), Box<dyn std::error::Error>> {
        let is_recording = Arc::new(Mutex::new(false));
        let (stream, stream_config, recorder_rx) = audio::recorder::init()?;
        let (resampled_rx, resampler_handle) = audio::resampler::init(
            recorder_rx,
            stream_config.sample_rate as usize,
            stream_config.input_channels,
        ).await?;
        let (stt_tx, audio_buffer_rx) = mpsc::channel::<Vec<f32>>(1);
        let capture_handle = init_audio_capture(resampled_rx, stt_tx, is_recording.clone()).await?;
        let (stt_rx, stt_handle) = audio::speech_to_text::init(audio_buffer_rx).await?;

        let pipeline = AudioPipeline {
            stream,
            resampler_handle,
            stt_handle,
            capture_handle,
            is_recording,
        };

        Ok((pipeline, stt_rx))
    }

    fn pause(&self) {
        match self.stream.pause() {
            Ok(()) => (),
            Err(e) => eprintln!("Failed to pause audio stream: {}", e),
        };
    }

    fn play(&self) {
        match self.stream.play() {
            Ok(()) => (),
            Err(e) => eprintln!("Failed to start audio stream: {}", e),
        }
    }

    /// Start stream and accumulate resampled audio into buffer.
    /// If buffer reaches timeout size, stop recording and transcribe.
    pub async fn start_recording(&self) {
        self.play();
        *self.is_recording.lock().await = true;
    }

    /// Stop stream, send accumulated audio_buffer to STT, and clear buffer.
    pub async fn stop_recording_and_transcribe(&self) {
        self.pause();
        *self.is_recording.lock().await = false;
    }
}

async fn init_audio_capture(
    mut rx: mpsc::Receiver<Vec<f32>>,
    tx: mpsc::Sender<Vec<f32>>,
    is_recording: Arc<Mutex<bool>>
) -> Result<JoinHandle<()>, Box<dyn std::error::Error>> {
    let handle = tokio::spawn(async move {
        let mut buffer = Vec::<f32>::with_capacity(16_000 * RECORDING_TIMEOUT_SECS as usize);

        loop {
            // todo: this is dumb?
            // todo2: this guy probably dies after exceeding buffer once. (or mutex deadlock?)
            match *is_recording.lock().await {
                true => {
                    while let Ok(data) = rx.try_recv() {
                        buffer.extend_from_slice(&data);
                        // If buffer length exceeds timeout, stop recording and send to STT
                        if buffer.len() >= 16_000 * RECORDING_TIMEOUT_SECS as usize {
                            tx.send(buffer.clone()).await.unwrap();
                            buffer.clear();
                            *is_recording.lock().await = false;
                        }
                    }
                },
                false => {
                    if !buffer.is_empty() {
                        tx.send(buffer.clone()).await.unwrap();
                        buffer.clear();
                    }
                },
            };
        }
    });

    Ok(handle)
}

