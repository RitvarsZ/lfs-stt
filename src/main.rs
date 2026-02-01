use std::{sync::mpsc};

use crate::insim_input::InsimCommand;

mod audio_input;
mod insim_input;
mod resampler;
mod stt;
mod ui;

pub const DEBUG_AUDIO_RESAMPLING: bool = true;
pub const USE_GPU: bool = true;
pub const MODEL_PATH: &str = "models/small.en.bin";
pub const INSIM_HOST: &str = "127.0.0.1";
pub const INSIM_PORT: &str = "29999";
pub const MESSAGE_PREVIEW_BTN_ID: u8 = 1;
pub const MESSAGE_PREVIEW_TOP: u8 = 100; // from 0 to 200
pub const MESSAGE_PREVIEW_TIMEOUT_SECS: u64 = 10;

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

    let mut is_recording = false;
    let mut message = String::from("");
    let mut message_timeout: Option<std::time::Instant> = None;

    loop {
        // Check for message preview timeout and clear message if reached.
        if let Some(timeout) = message_timeout && std::time::Instant::now() >= timeout{
            ui::clear_message_preview(&mut conn);
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
                    let btn = ui::get_message_preview(message.clone());
                    match conn.write(insim::Packet::Btn(btn)) {
                        Ok(_) => {
                            let t = std::time::Instant::now().checked_add(std::time::Duration::from_secs(MESSAGE_PREVIEW_TIMEOUT_SECS));
                            if let Some(t) = t {
                                message_timeout = Some(t);
                            } else {
                                message_timeout = None;
                                println!("Error setting message preview timeout");
                            }
                        },
                        Err(e) => {
                            println!("Error sending message preview: {:?}", e);
                            message.clear();
                            message_timeout = None;
                        }
                    }
                }
            };
        }


        if let Ok(cmd) = insim_receiver.try_recv() {
            match cmd {
                InsimCommand::ToggleRecording => {
                    is_recording = !is_recording;
                    if is_recording {
                        println!("Started recording...");
                        audio_capture.start_stream()?;
                    } else {
                        println!("Stopped recording...");
                        audio_capture.pause_stream()?;
                    }
                    *stt_ctx.is_recording.lock().unwrap() = is_recording;
                },
                InsimCommand::AcceptMessage => {
                    if message.is_empty() { continue; }
                    let packet = insim::insim::Mst{
                        reqi: insim::identifiers::RequestId::from(1),
                        msg: message.clone(),
                    };
                    conn.write(insim::Packet::Mst(packet))?;
                    ui::clear_message_preview(&mut conn);
                    message.clear();
                    message_timeout = None;
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

