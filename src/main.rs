use audioadapter_buffers::direct::InterleavedSlice;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use rubato::{Fft, Resampler};
use std::time::Duration;
use whisper_rs::{convert_stereo_to_mono_audio};

use crate::stt::maybe_dump_buffer_to_wav;
mod stt;

pub const DEBUG_AUDIO_RESAMPLING: bool = false;
pub const USE_GPU: bool = true;
pub const MODEL_PATH: &str = "models/medium.en.bin";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🎮 Press SPACE to toggle recording. Press ESC to quit.");

    // -----------------------------
    // 1 Setup speech-to-text
    // -----------------------------
    let stt_ctx = stt::SttContext::init_stt();

    // -----------------------------
    // 2 Setup audio capture
    // -----------------------------
    let host = cpal::default_host();
    let device = host.default_input_device().expect("No input device");
    let input_config = device.default_input_config()?;
    let input_channels = input_config.channels() as usize;
    let sample_rate = input_config.sample_rate() as usize;

    let audio_data_clone = stt_ctx.audio_data.clone();
    let recording_clone = stt_ctx.recording.clone();

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
    // 3 Resampler setup (16kHz target)
    // -----------------------------
    let mut resampler = Fft::<f32>::new(sample_rate, 16_000, 1024, 2, 1, rubato::FixedSync::Both)?;

    // -----------------------------
    // 4 Push-to-talk toggle loop
    // -----------------------------
    let mut is_recording = false;

    loop {
        // -----------------------------
        // 5 Read logs from STT thread
        // -----------------------------
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
                        stt_ctx.audio_data.lock().unwrap().clear();
                        *stt_ctx.recording.lock().unwrap() = true;
                        is_recording = true;
                    } else {
                        println!("Converting input to 16k mono");
                        *stt_ctx.recording.lock().unwrap() = false;
                        let raw_samples = stt_ctx.audio_data.lock().unwrap().clone();
                        let mono = convert_stereo_to_mono_audio(&raw_samples).expect("should be no half samples missing");

                        let nbr_input_frames = mono.len();
                        let input_adapter = InterleavedSlice::new(&mono, 1, nbr_input_frames).unwrap();
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

                        maybe_dump_buffer_to_wav(&outdata)?;

                        println!("🛑 Sending for transcription...");
                        stt_ctx.audio_tx.send(outdata).unwrap();
                        is_recording = false;
                    }
                }
                _ => {}
            }
        }
    }

    Ok(())
}

