use anyhow::Result;
use audioadapter_buffers::direct::InterleavedSlice;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use rubato::{Fft, Resampler};
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
    params.use_gpu(true); // tiny/small models are fast on CPU
    params.gpu_device(1);
    let ctx = Arc::new(WhisperContext::new_with_params("models/medium.en.bin", params)?);

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

        let mut full_params = FullParams::new(SamplingStrategy::Greedy { best_of: 8 });
        full_params.set_language(Some("en"));
        full_params.set_print_special(false);
        full_params.set_print_progress(false);
        full_params.set_print_realtime(false);
        full_params.set_print_timestamps(false);

        while let Ok(samples) = audio_rx.recv() {
            log_tx.send(format!("📦 Received {} samples for transcription", samples.len())).unwrap();

            if let Err(err) = state.full(full_params.clone(), &samples) {
                log_tx.send(format!("❌ Transcription error: {:?}", err)).unwrap();
                continue;
            }

            let mut text = String::new();
            let n_segments = state.full_n_segments();
            for i in 0..n_segments {
                if let Some(segment) = state.get_segment(i) && let Ok(segment) = segment.to_str() {
                    text.push_str(segment);
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
    let input_channels = input_config.channels() as usize;
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
    let mut resampler = Fft::<f32>::new(sample_rate, 16_000, 1024, 2, 1, rubato::FixedSync::Both)?;

    // -----------------------------
    // 6️⃣ Push-to-talk toggle loop
    // -----------------------------
    let mut is_recording = false;

    loop {
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
                        audio_data.lock().unwrap().clear();
                        *recording.lock().unwrap() = true;
                        is_recording = true;
                    } else {
                        println!("Converting input to 16k mono");
                        *recording.lock().unwrap() = false;
                        let raw_samples = audio_data.lock().unwrap().clone();
                        let nbr_input_frames = raw_samples.len();
                        let input_adapter = InterleavedSlice::new(&raw_samples, 1, nbr_input_frames).unwrap();

                        let mut outdata = vec![0.0; nbr_input_frames * 16_000 / sample_rate + 256];
                        let nbr_out_frames = outdata.len();
                        println!("nbr_input_frames: {}, out_frames: {}, input_channels: {}", nbr_input_frames, nbr_out_frames, input_channels);
                        let mut output_adapter = InterleavedSlice::new_mut(&mut outdata, 1, nbr_out_frames).unwrap();

                        let mut indexing = rubato::Indexing {
                            input_offset: 0,
                            output_offset: 0,
                            active_channels_mask: None,
                            partial_len: None,
                        };
                        let mut input_frames_left = nbr_input_frames;
                        let mut input_frames_next = resampler.input_frames_next();

                        // Loop over all full chunks.
                        // There will be some unprocessed input frames left after the last full chunk.
                        // see the `process_f64` example for how to handle those
                        // using `partial_len` of the indexing struct.
                        // It is also possible to use the `process_all_into_buffer` method
                        // to process the entire file (including any last partial chunk) with a single call.
                        while input_frames_left >= input_frames_next {
                            let (frames_read, frames_written) = resampler
                                .process_into_buffer(&input_adapter, &mut output_adapter, Some(&indexing))
                                .unwrap();

                            indexing.input_offset += frames_read;
                            indexing.output_offset += frames_written;
                            input_frames_left -= frames_read;
                            input_frames_next = resampler.input_frames_next();
                        }

                        println!("🛑 Sending for transcription...");
                        audio_tx.send(outdata).unwrap();
                        is_recording = false;
                    }
                }
                _ => {}
            }
        }

        // -----------------------------
        // 7️⃣ Read logs from STT thread
        // -----------------------------
        while let Ok(msg) = log_rx.try_recv() {
            println!("{}", msg);
        }
    }

    Ok(())
}

