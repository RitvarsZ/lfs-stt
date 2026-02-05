use std::pin::Pin;

use insim::builder::InsimTask;
use tokio::time::Sleep;
use tracing::{debug, error, info};

use crate::{MAX_MESSAGE_LEN, MESSAGE_PREVIEW_TIMEOUT_SECS, audio::{audio_pipeline::{AudioPipeline}, speech_to_text::{SttMessage, SttMessageType}}, insim_io::InsimEvent};

const STATE_ID: u8 = 1;
const PREVIEW_ID: u8 = 2;
const CHANNEL_ID: u8 = 3;
const UI_SCALE: u8 = 5;
pub const UI_OFFSET_TOP: u8 = 170;
pub const UI_OFFSET_LEFT: u8 = 10;

#[derive(Clone, Debug)]
pub struct ChatChannel {
    pub display: String,
    pub prefix: String,
}

impl PartialEq for ChatChannel {
    fn eq(&self, other: &Self) -> bool {
        self.prefix == other.prefix
    }
}

#[derive(Debug, Clone, Copy)]
pub enum UiState {
    Idle,
    Recording,
    Processing,
    Stopped,
}

#[derive(Debug)]
pub enum UiEvent {
    UpdatePreview(String),
    UpdateState(UiState),
    UpdateChannel(ChatChannel),
    ClearPreview,
    RemoveAllBtns,
}

pub struct UiContext {
    message_timeout: Option<Pin<Box<Sleep>>>,
    state: UiState,
    message: String,
    update_queue: Vec<UiEvent>,
    chat_channels: Vec<ChatChannel>,
    active_channel: ChatChannel,
}

impl Default for UiContext {
    fn default() -> Self {
        let chat_channels = vec![
            ChatChannel{display: "/say".to_string(), prefix: "".to_string()},
            ChatChannel{display: "^5!local".to_string(), prefix: "!l".to_string()},
        ];

        UiContext {
            state: UiState::Stopped,
            message: String::from(""),
            message_timeout: None,
            update_queue: vec![],
            active_channel: chat_channels[0].clone(),
            chat_channels,
        }
    }
}

impl UiContext {
    pub async fn clear_message_timeout(&mut self) {
        if let Some(t) = &mut self.message_timeout {
            t.as_mut().await;
            self.update_queue.push(UiEvent::ClearPreview);
            self.message.clear();
            self.message_timeout = None;
        }
    }

    pub async fn dispatch_ui_events(&mut self, insim: InsimTask) {
        if !self.update_queue.is_empty() {
            debug!("Dispatching {} UI events", self.update_queue.len());
        }

        while let Some(event) = self.update_queue.pop() {
            match event {
                UiEvent::UpdatePreview(message) => {
                    let _ = insim.send(insim::Packet::Btn(get_message_preview_btn(message))).await;
                },
                UiEvent::ClearPreview => {
                    let bfn = insim::insim::Bfn {
                        subt: insim::insim::BfnType::DelBtn,
                        reqi: insim::identifiers::RequestId::from(1),
                        clickid: insim::identifiers::ClickId::from(PREVIEW_ID),
                        clickmax: 0,
                        ucid: insim::identifiers::ConnectionId::LOCAL,
                        ..Default::default()
                    };
                    let _ = insim.send(insim::Packet::Bfn(bfn)).await;
                },
                UiEvent::UpdateState(state) => {
                    let _ = insim.send(insim::Packet::Btn(get_state_btn(state))).await;
                },
                UiEvent::RemoveAllBtns => {
                    let _ = insim.send(insim::Packet::Bfn(insim::insim::Bfn{
                        subt: insim::insim::BfnType::Clear,
                        reqi: insim::identifiers::RequestId::from(1),
                        clickid: insim::identifiers::ClickId::from(0),
                        clickmax: 0,
                        ucid: insim::identifiers::ConnectionId::LOCAL,
                        ..Default::default()
                    })).await;
                },
                UiEvent::UpdateChannel(channel) => {
                    let _ = insim.send(insim::Packet::Btn(get_channel_btn(channel))).await;
                }
            };
        }
    }

    pub fn handle_stt_message(&mut self, msg: SttMessage) {
        match msg.msg_type {
            SttMessageType::Log => {
                info!("{}", msg);
            },
            SttMessageType::TranscriptionError => {
                error!("{}", msg);
            },
            SttMessageType::TranscriptionResult => {
                info!("{}", msg);
                self.message = msg.content;
                self.state = UiState::Idle;
                self.update_queue.push(UiEvent::UpdateState(self.state));
                self.update_queue.push(UiEvent::UpdatePreview(self.message.clone()));
                self.message_timeout = Some(Box::pin(
                    tokio::time::sleep(std::time::Duration::from_secs(MESSAGE_PREVIEW_TIMEOUT_SECS))
                ));
            },
        };
    }

    pub async fn handle_insim_event(&mut self, event: InsimEvent, insim: InsimTask, audio_pipeline: &mut AudioPipeline) {
        match event {
            InsimEvent::IsInGame(is_in_game) => {
                if is_in_game {
                    match self.state {
                        UiState::Stopped => {
                            info!("Detected in-game state, starting STT.");
                            self.state = UiState::Idle;
                            if !self.message.is_empty() {
                                self.update_queue.push(UiEvent::UpdatePreview(self.message.clone()));
                            }
                            self.update_queue.push(UiEvent::UpdateState(self.state));
                            self.update_queue.push(UiEvent::UpdateChannel(self.active_channel.clone()));
                        },
                        _ => { /* No state change */ }
                    };
                } else {
                    match self.state {
                        UiState::Stopped => { /* No state change */ }
                        _ => {
                            info!("Detected not in-game state, stopping STT.");
                            self.state = UiState::Stopped;
                            self.update_queue.push(UiEvent::RemoveAllBtns);
                        }
                    };
                }
            },
            InsimEvent::ToggleRecording => {
                match self.state {
                    UiState::Processing => {},
                    UiState::Stopped => {},
                    UiState::Idle => {
                        info!("Started recording...");
                        self.state = UiState::Recording;
                        self.update_queue.push(UiEvent::UpdateState(self.state));
                        audio_pipeline.start_recording().await;
                    },
                    UiState::Recording => {
                        info!("Stopped recording...");
                        self.state = UiState::Processing;
                        self.update_queue.push(UiEvent::UpdateState(self.state));
                        audio_pipeline.stop_recording_and_transcribe().await;
                    },
                };
            },
            InsimEvent::AcceptMessage => {
                if self.message.is_empty() { return; }

                if let UiState::Idle = self.state {
                    // Split message into chunks of MAX_MESSAGE_LEN and send each chunk as a separate Msx packet.
                    let mut messages: Vec<String> = self.message.chars()
                        .collect::<Vec<_>>()
                        .chunks(MAX_MESSAGE_LEN - self.active_channel.prefix.len())
                        .map(|chunk| {
                            let mut msg = format!("{} ", self.active_channel.prefix);
                            msg.push_str(chunk.iter().collect::<String>().as_str());
                            msg
                        })
                        .rev()
                        .collect();

                    while let Some(part) = messages.pop() {
                        let msg = insim::insim::Msx{
                            reqi: insim::identifiers::RequestId::from(1),
                            msg: part.to_string(),
                        };
                        let _ = insim.send(insim::Packet::Msx(msg)).await;
                    }

                    self.update_queue.push(UiEvent::ClearPreview);
                    self.message.clear();
                    self.message_timeout = None;
                };
            },
            InsimEvent::NextChannel => {
                let current_index = self.chat_channels.iter().position(|c| c == &self.active_channel).unwrap_or(0);
                let next_index = (current_index + 1) % self.chat_channels.len();
                self.active_channel = self.chat_channels[next_index].clone();
                self.update_queue.push(UiEvent::UpdateChannel(self.active_channel.clone()));
            },
            InsimEvent::PeviousChannel => {
                let current_index = self.chat_channels.iter().position(|c| c == &self.active_channel).unwrap_or(0);
                let previous_index = if current_index == 0 {
                    self.chat_channels.len() - 1
                } else {
                    current_index - 1
                };
                self.active_channel = self.chat_channels[previous_index].clone();
                self.update_queue.push(UiEvent::UpdateChannel(self.active_channel.clone()));
            },
        }
    }
}

fn get_state_btn(state: UiState) -> insim::insim::Btn {
    let text = match state {
        UiState::Idle => "^2•",
        UiState::Recording => "^1•",
        UiState::Processing => "^3•",
        UiState::Stopped => "",
    };

    insim::insim::Btn{
        text: insim::core::string::escaping::escape(text).to_string(),
        t: UI_OFFSET_TOP,
        w: UI_SCALE,
        h: UI_SCALE,
        l: UI_OFFSET_LEFT,
        reqi: insim::identifiers::RequestId::from(1),
        ucid: insim::identifiers::ConnectionId::LOCAL,
        clickid: insim::identifiers::ClickId::from(STATE_ID),
        bstyle: insim::insim::BtnStyle{
            colour: insim::insim::BtnStyleColour::NotEditable,
            flags: insim::insim::BtnStyleFlags::LIGHT,
        },
        ..Default::default()
    }
}

/// depending on charaters used, width may vary
/// todo: this is not too accurate. Do we have to look at specific chars?
fn msg_to_btn_width(message: String) -> u8 {
    let len = insim::core::string::colours::strip(message.as_str()).len();
    let width = (len as f32 * 0.75).ceil() as u8 + 3;
    width.clamp(1, 200)
}

fn get_message_preview_btn(message: String) -> insim::insim::Btn {
    let text = insim::core::string::escaping::escape(format!("^3{}", message).as_str()).to_string();
    insim::insim::Btn{
        text,
        t: UI_OFFSET_TOP,
        w: msg_to_btn_width(message.clone()),
        h: UI_SCALE,
        l: UI_OFFSET_LEFT + UI_SCALE, // next to state
        reqi: insim::identifiers::RequestId::from(1),
        ucid: insim::identifiers::ConnectionId::LOCAL,
        clickid: insim::identifiers::ClickId::from(PREVIEW_ID),
        bstyle: insim::insim::BtnStyle{
            colour: insim::insim::BtnStyleColour::NotEditable,
            flags: insim::insim::BtnStyleFlags::LIGHT | insim::insim::BtnStyleFlags::LEFT,
        },
        ..Default::default()
    }
}

fn get_channel_btn(channel: ChatChannel) -> insim::insim::Btn {
    let text = insim::core::string::escaping::escape(channel.display.as_str()).to_string();

    insim::insim::Btn{
        text,
        t: UI_OFFSET_TOP + UI_SCALE,
        l: UI_OFFSET_LEFT,
        h: UI_SCALE,
        w: msg_to_btn_width(channel.display.to_string()),
        reqi: insim::identifiers::RequestId::from(1),
        ucid: insim::identifiers::ConnectionId::LOCAL,
        clickid: insim::identifiers::ClickId::from(CHANNEL_ID),
        bstyle: insim::insim::BtnStyle{
            colour: insim::insim::BtnStyleColour::NotEditable,
            flags: insim::insim::BtnStyleFlags::LIGHT | insim::insim::BtnStyleFlags::LEFT,
        },
        ..Default::default()
    }
}

