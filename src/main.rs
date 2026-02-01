use std::{sync::mpsc};

use crate::{insim_io::{InsimEvent}, ui::{UiEvent, UiState, dispatch_ui_events}};

mod audio_input;
mod insim_io;
mod resampler;
mod stt;
mod ui;

pub const DEBUG_AUDIO_RESAMPLING: bool = false;
pub const USE_GPU: bool = true;
pub const MODEL_PATH: &str = "models/small.en.bin";
pub const INSIM_HOST: &str = "127.0.0.1";
pub const INSIM_PORT: &str = "29999";
pub const MESSAGE_PREVIEW_TIMEOUT_SECS: u64 = 20;
pub const RECORDING_TIMEOUT_SECS: u8 = 10;
pub const MAX_MESSAGE_LEN: usize = 95;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // From input to resampler
    // From resampler to stt
    let (audio_tx, audio_rx) = mpsc::channel::<Vec<f32>>();
    let (resampled_tx, resampled_rx) = mpsc::channel::<Vec<f32>>();
    let (insim_event_tx, mut insim_event_rx) = tokio::sync::mpsc::channel::<InsimEvent>(1);
    let (insim_cmd_tx, insim_cmd_rx) = tokio::sync::mpsc::channel::<insim::Packet>(1);

    insim_io::init_message_io(insim_event_tx, insim_cmd_rx);

    let mut audio_capture = audio_input::AudioStreamContext::new(audio_tx)?;
    resampler::init_resampler(
        audio_rx,
        resampled_tx,
        audio_capture.sample_rate,
        audio_capture.input_channels
    );
    let stt_ctx = stt::SttContext::new();
    stt::start_stt_worker(&stt_ctx, resampled_rx);

    let mut ui_state: UiState = UiState::Stopped;
    let mut message = String::from("");
    let mut message_timeout: Option<std::time::Instant> = None;
    let mut ui_update_queue: Vec<ui::UiEvent> = vec![];

    loop {
        if !ui_update_queue.is_empty() {
            println!("Dispatching {} UI events", ui_update_queue.len());
            dispatch_ui_events(insim_cmd_tx.clone(), &mut ui_update_queue).await;
        }
        // Check for message preview timeout and clear message if reached.
        if let Some(timeout) = message_timeout && std::time::Instant::now() >= timeout{
            ui_update_queue.push(UiEvent::ClearPreview);
            message.clear();
            message_timeout = None;
        }

        // Check if there are any messages from the STT thread.
        if let Ok(msg) = stt_ctx.log_rx.try_recv() {
            match msg.msg_type {
                stt::SttThreadMessageType::Log |
                stt::SttThreadMessageType::TranscriptionError => {
                    println!("{}", msg);
                },
                stt::SttThreadMessageType::TranscriptionResult => {
                    println!("{}", msg);
                    message = msg.content;
                    ui_state = UiState::Idle;
                    ui_update_queue.push(UiEvent::UpdateState(ui_state));
                    ui_update_queue.push(UiEvent::UpdatePreview(message.clone()));
                    let t = std::time::Instant::now().checked_add(std::time::Duration::from_secs(MESSAGE_PREVIEW_TIMEOUT_SECS));
                    if let Some(t) = t {
                        message_timeout = Some(t);
                    } else {
                        message_timeout = None;
                        println!("Error setting message preview timeout");
                    }
                },
                stt::SttThreadMessageType::RecordingTimeoutReached => {
                    println!("{}", msg);
                    ui_state = UiState::Processing;
                    ui_update_queue.push(UiEvent::UpdateState(ui_state));
                    audio_capture.pause_stream()?;
                    *stt_ctx.is_recording.lock().unwrap() = false;
                }
            };
        }

        // Check received insim events.
        if let Ok(cmd) = insim_event_rx.try_recv() {
            match cmd {
                InsimEvent::IsInGame(is_in_game) => {
                    if is_in_game {
                        match ui_state {
                            UiState::Stopped => {
                                println!("Detected in-game state, starting STT.");
                                ui_state = UiState::Idle;
                                ui_update_queue.push(UiEvent::UpdatePreview(message.clone()));
                                ui_update_queue.push(UiEvent::UpdateState(ui_state));
                            },
                            _ => { /* No state change */ }
                        };
                    } else {
                        match ui_state {
                            UiState::Stopped => { /* No state change */ }
                            _ => {
                                println!("Detected not in-game state, stopping STT.");
                                ui_state = UiState::Stopped;
                                ui_update_queue.push(UiEvent::RemoveAllBtns);
                            }
                        };
                    }
                },
                InsimEvent::ToggleRecording => {
                    match ui_state {
                        UiState::Stopped => {
                            continue;
                        },
                        UiState::Idle => {
                            println!("Started recording...");
                            ui_state = UiState::Recording;
                            ui_update_queue.push(UiEvent::UpdateState(ui_state));
                            audio_capture.start_stream()?;
                            *stt_ctx.is_recording.lock().unwrap() = true;
                        },
                        UiState::Recording => {
                            println!("Stopped recording...");
                            ui_state = UiState::Processing;
                            ui_update_queue.push(UiEvent::UpdateState(ui_state));
                            audio_capture.pause_stream()?;
                            *stt_ctx.is_recording.lock().unwrap() = false;
                        },
                        UiState::Processing => { continue; },
                    };
                },
                InsimEvent::AcceptMessage => {
                    if message.is_empty() { continue; }

                    match ui_state {
                        UiState::Idle => {
                            // Split message into chunks of MAX_MESSAGE_LEN and send each chunk as a separate Msx packet.
                            let mut messages: Vec<String> = message.chars()
                                .collect::<Vec<_>>()
                                .chunks(MAX_MESSAGE_LEN)
                                .map(|chunk| chunk.iter().collect())
                                .rev()
                                .collect();

                            while let Some(part) = messages.pop() {
                                let msg = insim::insim::Msx{
                                    reqi: insim::identifiers::RequestId::from(1),
                                    msg: part.to_string(),
                                };
                                let _ = insim_cmd_tx.send(insim::Packet::Msx(msg)).await;
                            }

                            ui_update_queue.push(UiEvent::ClearPreview);
                            message.clear();
                            message_timeout = None;
                        },
                        _ => { continue; }
                    };
                },
                InsimEvent::NextChannel => {
                    todo!("Implement channel switching");
                },
                InsimEvent::PeviousChannel => {
                    todo!("Implement channel switching");
                },
            }
        }
    }
}

