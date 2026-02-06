use std::sync::{Arc, atomic::AtomicBool};

use cpal::{SampleRate, Stream, traits::{DeviceTrait, HostTrait, StreamTrait}};
use tokio::{sync::mpsc::{self, Receiver}};
use tracing::{error, info, warn};

use crate::audio::{AudioBackendError, audio_pipeline::CaptureMsg};

pub struct AudioInputConfig {
    pub input_channels: usize,
    pub sample_rate: SampleRate,
}

pub fn init(
    is_recording: Arc<AtomicBool>,
) -> Result<(Stream, AudioInputConfig, Receiver<CaptureMsg>), AudioBackendError> {
    let (audio_tx, audio_rx) = mpsc::channel::<CaptureMsg>(10);

    let host = cpal::default_host();
    let device = match host.default_input_device() {
        Some(device) => device,
        None => return Err(AudioBackendError::NoInputDevice),
    };
    let input_config = device.default_input_config()?;
    let input_channels = input_config.channels() as usize;
    if (input_channels != 1) && (input_channels != 2) {
        return Err(AudioBackendError::UnsupportedInputChannels);
    }

    let sample_rate = input_config.sample_rate();
    let audio_tx_clone = audio_tx.clone();
    let stream = device.build_input_stream(
        &input_config.into(),
        move |data: &[f32], _| {
            if is_recording.load(std::sync::atomic::Ordering::Relaxed) {
                match audio_tx.try_send(CaptureMsg::Audio(data.to_vec())) {
                    Ok(_) => (),
                    Err(e) => error!("Failed to send audio data: {}", e),
                };
            }
        },
        move |err| {
            match err {
                cpal::StreamError::DeviceNotAvailable => error!("Audio input device not available"),
                cpal::StreamError::StreamInvalidated => error!("Audio stream invalidated"),
                cpal::StreamError::BufferUnderrun => error!("Audio buffer underrun"),
                cpal::StreamError::BackendSpecific { err } => error!("Audio stream error: {}", err),
            }
            let _ = audio_tx_clone.blocking_send(CaptureMsg::Exit);
        },
        None,
    )?;

    match stream.play() {
        Ok(()) => (),
        Err(e) => error!("Failed to start audio stream: {}", e),
    }

    let name = match device.description() {
        Ok(desc) => desc.to_string(),
        Err(_) => {
            warn!("Failed to get audio input device name");
            "Unknown Device".to_string()
        }
    };
    info!("Using input device: {}", name);

    let config = AudioInputConfig {
        input_channels,
        sample_rate,
    };

    Ok((stream, config, audio_rx))
}
