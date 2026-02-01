use std::{sync::mpsc};

use crate::{insim_input::InsimCommand, ui::{UiEvent, UiState, dispatch_ui_events}};

mod audio_input;
mod insim_input;
mod resampler;
mod stt;
mod ui;

pub const DEBUG_AUDIO_RESAMPLING: bool = false;
pub const USE_GPU: bool = true;
pub const MODEL_PATH: &str = "models/small.en.bin";
pub const INSIM_HOST: &str = "127.0.0.1";
pub const INSIM_PORT: &str = "29999";
pub const MESSAGE_PREVIEW_TOP: u8 = 150; // from 0 to 200
pub const MESSAGE_PREVIEW_TIMEOUT_SECS: u64 = 20;
const MAX_MESSAGE_LEN: usize = 95;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = insim::tcp(format!("{}:{}", INSIM_HOST, INSIM_PORT)).connect_blocking()?;
    // From input to resampler
    // From resampler to stt
    let (audio_sender, audio_receiver) = mpsc::channel::<Vec<f32>>();
    let (resampled_sender, resampled_receiver) = mpsc::channel::<Vec<f32>>();
    let (insim_sender, insim_receiver) = mpsc::channel::<InsimCommand>();

    insim_input::init_message_reader(insim_sender);

    let mut audio_capture = audio_input::AudioStreamContext::new(audio_sender)?;
    resampler::init_resampler(
        audio_receiver,
        resampled_sender,
        audio_capture.sample_rate,
        audio_capture.input_channels
    );
    let stt_ctx = stt::SttContext::new();
    stt::start_stt_worker(&stt_ctx, resampled_receiver);

    let mut ui_state: UiState = UiState::Idle;
    let mut message = String::from("");
    let mut message_timeout: Option<std::time::Instant> = None;
    let mut ui_update_queue: Vec<ui::UiEvent> = vec![
        UiEvent::UpdatePreview(message.clone()),
        UiEvent::UpdateState(ui_state),
    ];

    loop {
        if !ui_update_queue.is_empty() {
            println!("{:?}", ui_update_queue);
        }
        dispatch_ui_events(&mut conn, &mut ui_update_queue);
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
                    message.truncate(MAX_MESSAGE_LEN);
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
                }
            };
        }


        if let Ok(cmd) = insim_receiver.try_recv() {
            match cmd {
                InsimCommand::ToggleRecording => {
                    match ui_state {
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
                InsimCommand::AcceptMessage => {
                    if message.is_empty() { continue; }

                    match ui_state {
                        UiState::Idle => {
                            let msg = insim::insim::Msx{
                                reqi: insim::identifiers::RequestId::from(1),
                                msg: message.clone(),
                            };
                            conn.write(insim::Packet::Msx(msg))?;
                            ui_update_queue.push(UiEvent::ClearPreview);
                            message.clear();
                            message_timeout = None;
                        },
                        _ => { continue; }
                    };
                },
                InsimCommand::NextChannel => {
                    todo!("Implement channel switching");
                },
                InsimCommand::PeviousChannel => {
                    todo!("Implement channel switching");
                },
            }
        }
    }
}

