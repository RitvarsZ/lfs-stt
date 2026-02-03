use std::pin::Pin;

use insim::builder::InsimTask;
use tokio::time::Sleep;

use crate::{MAX_MESSAGE_LEN, MESSAGE_PREVIEW_TIMEOUT_SECS, audio::{audio_pipeline::{AudioPipeline}, speech_to_text::{SttMessage, SttMessageType}}, insim_io::InsimEvent};

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
    ClearPreview,
    UpdateState(UiState),
    RemoveAllBtns,
}

pub const STATE_ID: u8 = 1;
pub const PREVIEW_ID: u8 = 2;

pub const UI_SCALE: u8 = 5;

pub const STATE_OFFSET_LEFT: u8 = 10;
pub const PREVIEW_OFFSET_LEFT: u8 = STATE_OFFSET_LEFT + UI_SCALE;
pub const PREVIEW_OFFEST_TOP: u8 = 170; // from 0 to 200

pub struct UiContext {
    pub state: UiState,
    pub message: String,
    pub message_timeout: Option<Pin<Box<Sleep>>>,
    pub update_queue: Vec<UiEvent>,
}

impl Default for UiContext {
    fn default() -> Self {
        UiContext {
            state: UiState::Stopped,
            message: String::from(""),
            message_timeout: None,
            update_queue: vec![],
        }
    }
}

impl UiContext {
    pub async fn dispatch_ui_events(&mut self, insim: InsimTask) {
        if !self.update_queue.is_empty() {
            println!("Dispatching {} UI events", self.update_queue.len());
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
                        inst: insim::insim::BtnInst::default(),
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
                        inst: insim::insim::BtnInst::default(),
                    })).await;
                },
            };
        }
    }

    pub fn handle_stt_message(&mut self, msg: SttMessage) {
        match msg.msg_type {
            SttMessageType::Log |
            SttMessageType::TranscriptionError => {
                println!("{}", msg);
            },
            SttMessageType::TranscriptionResult => {
                println!("{}", msg);
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
                            println!("Detected in-game state, starting STT.");
                            self.state = UiState::Idle;
                            self.update_queue.push(UiEvent::UpdatePreview(self.message.clone()));
                            self.update_queue.push(UiEvent::UpdateState(self.state));
                        },
                        _ => { /* No state change */ }
                    };
                } else {
                    match self.state {
                        UiState::Stopped => { /* No state change */ }
                        _ => {
                            println!("Detected not in-game state, stopping STT.");
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
                        println!("Started recording...");
                        self.state = UiState::Recording;
                        self.update_queue.push(UiEvent::UpdateState(self.state));
                        audio_pipeline.start_recording().await;
                    },
                    UiState::Recording => {
                        println!("Stopped recording...");
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
                        .chunks(MAX_MESSAGE_LEN)
                        .map(|chunk| chunk.iter().collect())
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
                todo!("Implement channel switching");
            },
            InsimEvent::PeviousChannel => {
                todo!("Implement channel switching");
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
        t: PREVIEW_OFFEST_TOP,
        w: UI_SCALE,
        h: UI_SCALE,
        l: STATE_OFFSET_LEFT,
        reqi: insim::identifiers::RequestId::from(1),
        ucid: insim::identifiers::ConnectionId::LOCAL,
        clickid: insim::identifiers::ClickId::from(STATE_ID),
        inst: insim::insim::BtnInst::default(),
        bstyle: insim::insim::BtnStyle{
            colour: insim::insim::BtnStyleColour::NotEditable,
            flags: insim::insim::BtnStyleFlags::LIGHT,
        },
        typein: None,
        caption: None,
        text: text.to_string(),
    }
}

// depending on charaters used, width may vary
fn msg_to_btn_width(message: String) -> u8 {
    let len = message.len();
    let width = (len as f32 * 0.75).ceil() as u8 + UI_SCALE;
    width.clamp(5, 200)
}

fn get_message_preview_btn(message: String) -> insim::insim::Btn {
    insim::insim::Btn{
        t: PREVIEW_OFFEST_TOP,
        w: msg_to_btn_width(message.clone()),
        h: UI_SCALE,
        l: PREVIEW_OFFSET_LEFT,
        reqi: insim::identifiers::RequestId::from(1),
        ucid: insim::identifiers::ConnectionId::LOCAL,
        clickid: insim::identifiers::ClickId::from(PREVIEW_ID),
        inst: insim::insim::BtnInst::default(),
        bstyle: insim::insim::BtnStyle{
            colour: insim::insim::BtnStyleColour::NotEditable,
            flags: insim::insim::BtnStyleFlags::LIGHT | insim::insim::BtnStyleFlags::LEFT,
        },
        typein: None,
        caption: None,
        text: format!("^3{}", message),
    }
}

