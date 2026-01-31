use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use std::{sync::mpsc, time::Duration};

use crate::{audio_input::AudioStreamContext, stt::start_stt_worker};

mod stt;
mod audio_input;
mod resampler;

pub const DEBUG_AUDIO_RESAMPLING: bool = true;
pub const USE_GPU: bool = true;
pub const MODEL_PATH: &str = "models/small.en.bin";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🎮 Press SPACE to toggle recording. Press ESC to quit.");

    // From input to resampler
    let (audio_sender, audio_receiver) = mpsc::channel::<Vec<f32>>();
    // From resampler to stt
    let (resampled_sender, resampled_receiver) = mpsc::channel::<Vec<f32>>();

    let mut audio_capture = AudioStreamContext::init_audio_capture(audio_sender)?;
    resampler::init_resampler(
        audio_receiver,
        resampled_sender,
        audio_capture.sample_rate,
        audio_capture.input_channels
    );
    let stt_ctx = stt::SttContext::new();
    start_stt_worker(&stt_ctx, resampled_receiver);

    let mut is_recording = false;

    loop {
        while let Ok(msg) = stt_ctx.log_rx.try_recv() {
            println!("{}", msg);
        }

        // Poll keys
        if !event::poll(Duration::from_millis(20))? {
            continue;
        }


        if let Event::Key(key) = event::read()? && key.kind == KeyEventKind::Press {
            match key.code {
                KeyCode::Char('q') => {
                    println!("👋 Exiting...");
                    break;
                },
                KeyCode::Char(' ') => {
                    if !is_recording {
                        println!("🎤 Recording...");
                        audio_capture.start_stream()?;
                        *stt_ctx.is_recording.lock().unwrap() = true;
                        is_recording = true;
                    } else {
                        println!("🛑 Sending for transcription...");
                        *stt_ctx.is_recording.lock().unwrap() = false;
                        is_recording = false;
                        audio_capture.pause_stream()?;
                    }
                }
                _ => {}
            }
        }
    }

    Ok(())
}

