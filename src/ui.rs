use crate::{MESSAGE_PREVIEW_BTN_ID, MESSAGE_PREVIEW_TOP};

pub fn get_message_preview(message: String) -> insim::insim::Btn {
    insim::insim::Btn{
        t: MESSAGE_PREVIEW_TOP,
        w: message.len() as u8,
        h: 4,
        l: 100 - (message.len() / 2) as u8,
        reqi: insim::identifiers::RequestId::from(1),
        ucid: insim::identifiers::ConnectionId::LOCAL,
        clickid: insim::identifiers::ClickId::from(MESSAGE_PREVIEW_BTN_ID),
        inst: insim::insim::BtnInst::default(),
        bstyle: insim::insim::BtnStyle{
            colour: insim::insim::BtnStyleColour::Ok,
            flags: insim::insim::BtnStyleFlags::default(),
        },
        typein: None,
        caption: None,
        text: format!("^3{}", message),
    }
}

pub fn get_button_clear_function() -> insim::insim::Bfn {
    insim::insim::Bfn {
        reqi: insim::identifiers::RequestId::from(1),
        subt: insim::insim::BfnType::Clear,
        ucid: insim::identifiers::ConnectionId::LOCAL,
        clickid: insim::identifiers::ClickId::from(MESSAGE_PREVIEW_BTN_ID),
        clickmax: MESSAGE_PREVIEW_BTN_ID,
        inst: insim::insim::BtnInst::default(),
    }
}

pub fn clear_message_preview(conn: &mut insim::net::blocking_impl::Framed) {
    match conn.write(insim::Packet::Bfn(get_button_clear_function())) {
        Ok(_) => {
            println!("Message preview cleared");
        },
        Err(e) => {
            println!("Error clearing message preview: {:?}", e);
        }
    }
}

