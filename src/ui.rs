#[derive(Debug, Clone, Copy)]
pub enum UiState {
    Idle,
    Recording,
    Processing,
}

#[derive(Debug)]
pub enum UiEvent {
    UpdatePreview(String),
    ClearPreview,
    UpdateState(UiState),
}

pub const STATE_ID: u8 = 1;
pub const PREVIEW_ID: u8 = 2;

pub const UI_SCALE: u8 = 4;

pub const STATE_OFFSET_LEFT: u8 = 4;
pub const PREVIEW_OFFSET_LEFT: u8 = STATE_OFFSET_LEFT + UI_SCALE;
pub const PREVIEW_OFFEST_TOP: u8 = 185; // from 0 to 200

pub fn dispatch_ui_events(
    conn: &mut insim::net::blocking_impl::Framed,
    events: &mut Vec<UiEvent>,
) {
    while let Some(event) = events.pop() {
        match event {
            UiEvent::UpdatePreview(message) => {
                let _ = conn.write(insim::Packet::Btn(get_message_preview_btn(message)));
            },
            UiEvent::ClearPreview => {
                let _ = conn.write(insim::Packet::Btn(get_message_preview_btn(String::from(""))));
            },
            UiEvent::UpdateState(state) => {
                let _ = conn.write(insim::Packet::Btn(get_state_btn(state)));
            },
        };
    }
}

fn get_state_btn(state: UiState) -> insim::insim::Btn {
    let text = match state {
        UiState::Idle => "^2•",
        UiState::Recording => "^1•",
        UiState::Processing => "^3•",
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
    let width = (len as f32 * 0.75).ceil() as u8;
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

