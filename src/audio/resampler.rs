use rubato::{
    Async, FixedAsync,
    SincInterpolationParameters, SincInterpolationType, WindowFunction,
    Resampler,
};
use whisper_rs::convert_stereo_to_mono_audio;
use tokio::{sync::mpsc::{Sender, Receiver}, task::JoinHandle};

use crate::audio::{AudioPipelineError, audio_pipeline::CaptureMsg};

pub async fn init(
    mut audio_rx: Receiver<CaptureMsg>,
    sample_rate: usize,
    input_channels: usize,
) -> Result<(Sender<CaptureMsg>, Receiver<CaptureMsg>, JoinHandle<Result<(), AudioPipelineError>>), AudioPipelineError> {
    let (resampled_tx, resampled_rx) = tokio::sync::mpsc::channel::<CaptureMsg>(10);
    let resampled_tx_clone = resampled_tx.clone();
    let handle = tokio::spawn(async move {
        let mut input_accum: Vec<f32> = Vec::new();

        let sinc_params = SincInterpolationParameters {
            sinc_len: 128,
            f_cutoff: 0.95,
            interpolation: SincInterpolationType::Linear,
            oversampling_factor: 256,
            window: WindowFunction::BlackmanHarris2,
        };

        let ratio = 16_000.0 / sample_rate as f64;
        let chunk_size = 1024;
        let mut resampler = match Async::<f32>::new_sinc(
            ratio,
            1.0, // no dynamic ratio range
            &sinc_params,
            chunk_size,
            1, // nbr_channels
            FixedAsync::Input,
        ) {
            Ok(r) => r,
            Err(e) => { return Err(AudioPipelineError::Resampler(e.into())); }
        };

        loop {
            let samples = match audio_rx.recv().await {
                Some(msg) => match msg {
                    CaptureMsg::Audio(samples) => { samples },
                    CaptureMsg::Stop => { continue; },
                    CaptureMsg::Exit => { return Ok(()) }, // exit signal, stop resampling task
                },
                None => { return Ok(()); },
            };

            let mono = match input_channels {
                1 => samples,
                2 => convert_stereo_to_mono_audio(&samples).expect("should be no half samples missing"),
                _ => panic!("Unsupported number of input channels: {}", input_channels),
            };

            input_accum.extend_from_slice(&mono);
            if input_accum.len() < 1024 {
                continue;
            }

            let mono: Vec<f32> = input_accum.drain(..1024).collect();

            // prep output adapters (same shape, but resized to max)
            let mut out = vec![0.0; resampler.output_frames_max()];

            // process into buffer
            let (_, out_frames) = match resampler
                .process_into_buffer(
                    &audioadapter_buffers::direct::InterleavedSlice::new(&mono, 1, mono.len()).unwrap(),
                    &mut audioadapter_buffers::direct::InterleavedSlice::new_mut(&mut out, 1, resampler.output_frames_max()).unwrap(),
                    None,
                ) {
                    Ok(r) => r,
                    Err(e) => { return Err(AudioPipelineError::Resampler(e.into())); }
                };

            out.truncate(out_frames);
            let _ = resampled_tx_clone.send(CaptureMsg::Audio(out)).await;
        }
    });

    Ok((resampled_tx, resampled_rx, handle))
}

