use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossterm::event::{self, Event, KeyCode};
use rubato::{FftFixedIn, Resampler};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;
use whisper_rs::{WhisperContext, WhisperContextParameters, FullParams, SamplingStrategy};

fn main() -> Result<()> {
    println!("🎮 Press SPACE to toggle recording. Press ESC to quit.");

    // -----------------------------
    // 1️⃣ Setup Whisper
    // -----------------------------
    let mut params = WhisperContextParameters::new();
    params.use_gpu(false); // tiny/small models are fast on CPU
    let ctx = Arc::new(WhisperContext::new_with_params("models/small.en.bin", params)?);

    // -----------------------------
    // 2️⃣ Shared audio buffer and recording flag
    // -----------------------------
    let audio_data = Arc::new(Mutex::new(Vec::<f32>::new()));
    let recording = Arc::new(Mutex::new(false));

    // -----------------------------
    // 3️⃣ Channels
    // -----------------------------
    let (audio_tx, audio_rx) = mpsc::channel::<Vec<f32>>(); // audio buffers to STT thread
    let (log_tx, log_rx) = mpsc::channel::<String>(); // logs + transcription to main thread

    // STT thread
    let ctx_clone = ctx.clone();
    thread::spawn(move || {
        let mut state = ctx_clone.create_state().unwrap();
        log_tx.send("✅ STT thread started".into()).unwrap();

        while let Ok(samples) = audio_rx.recv() {
            log_tx.send(format!("📦 Received {} samples for transcription", samples.len())).unwrap();

            let mut full_params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
            full_params.set_language(Some("en"));
            full_params.set_n_threads(4);
            full_params.set_print_special(false);
            full_params.set_print_progress(false);

            if let Err(err) = state.full(full_params, &samples) {
                log_tx.send(format!("❌ Transcription error: {:?}", err)).unwrap();
                continue;
            }

            let mut text = String::new();
            let n_segments = state.full_n_segments().unwrap_or(0);
            for i in 0..n_segments {
                if let Ok(segment) = state.full_get_segment_text(i) {
                    text.push_str(&segment);
                }
            }

            log_tx.send(format!("📝 Transcribed text: {}", text.trim())).unwrap();
        }
    });

    // -----------------------------
    // 4️⃣ Setup audio capture
    // -----------------------------
    let host = cpal::default_host();
    let device = host.default_input_device().expect("No input device");
    let input_config = device.default_input_config()?;
    let sample_rate = input_config.sample_rate().0 as usize;

    let audio_data_clone = audio_data.clone();
    let recording_clone = recording.clone();

    let stream = device.build_input_stream(
        &input_config.into(),
        move |data: &[f32], _| {
            if *recording_clone.lock().unwrap() {
                audio_data_clone.lock().unwrap().extend_from_slice(data);
            }
        },
        move |err| eprintln!("Audio error: {:?}", err),
        None,
    )?;

    stream.play()?;

    // -----------------------------
    // 5️⃣ Resampler setup (16kHz target)
    // -----------------------------
    let mut resampler = FftFixedIn::<f32>::new(sample_rate, 16_000, 1024, 2, 1)?;

    // -----------------------------
    // 6️⃣ Push-to-talk toggle loop
    // -----------------------------
    let mut is_recording = false;

    loop {
        // Poll keys
        if event::poll(Duration::from_millis(20))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Esc => break,
                    KeyCode::Char(' ') => {
                        if !is_recording {
                            println!("🎤 Recording...");
                            audio_data.lock().unwrap().clear();
                            *recording.lock().unwrap() = true;
                            is_recording = true;
                        } else {
                            println!("🛑 Sending for transcription...");
                            *recording.lock().unwrap() = false;
                            let raw_samples = audio_data.lock().unwrap().clone();

                            // Resample to 16kHz
                            let resampled = resampler.process(&[raw_samples.clone()], None).unwrap();
                            let resampled_flat: Vec<f32> = resampled.concat();

                            audio_tx.send(resampled_flat).unwrap();
                            is_recording = false;
                        }
                    }
                    _ => {}
                }
            }
        }

        // -----------------------------
        // 7️⃣ Read logs from STT thread
        // -----------------------------
        while let Ok(msg) = log_rx.try_recv() {
            println!("{}", msg);
        }
    }
}

