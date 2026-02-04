use std::sync::{Arc, atomic::AtomicBool};

use cpal::{SampleRate, Stream, traits::{DeviceTrait, HostTrait, StreamTrait}};
use tokio::{sync::mpsc::{self, Receiver}};
use tracing::{error, info};

use crate::audio::audio_pipeline::CaptureMsg;

pub struct AudioInputConfig {
    pub input_channels: usize,
    pub sample_rate: SampleRate,
}

pub fn init(
    is_recording: Arc<AtomicBool>,
) -> Result<(Stream, AudioInputConfig, Receiver<CaptureMsg>), Box<dyn std::error::Error>> {
    let (audio_tx, audio_rx) = mpsc::channel::<CaptureMsg>(10);

    let host = cpal::default_host();
    let device = host.default_input_device().expect("No input device");
    let input_config = match device.default_input_config() {
        Ok(config) => config,
        Err(e) => return Err(format!("Failed to get default input config: {}", e).into()),
    };
    let input_channels = input_config.channels() as usize;
    if (input_channels != 1) && (input_channels != 2) {
        return Err(format!("Unsupported number of input channels: {}. Only mono and stereo are supported.", input_channels).into());
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
            let _ = audio_tx_clone.send(CaptureMsg::Error(format!("Audio stream error: {}", err)));
        },
        None,
    )?;

    match stream.play() {
        Ok(()) => (),
        Err(e) => error!("Failed to start audio stream: {}", e),
    }

    info!("Using input device: {}", device.description()?);

    let config = AudioInputConfig {
        input_channels,
        sample_rate,
    };

    Ok((stream, config, audio_rx))
}
