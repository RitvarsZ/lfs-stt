use crate::{INSIM_HOST, INSIM_PORT};

#[allow(dead_code)]
pub enum InsimCommand {
    ToggleRecording,
    AcceptMessage,
    NextChannel,
    PeviousChannel,
}

impl InsimCommand {
    pub fn from_string(cmd: String) -> Option<InsimCommand> {
        match cmd.as_str() {
            "stt talk" => Some(InsimCommand::ToggleRecording),
            "stt accept" => Some(InsimCommand::AcceptMessage),
            _ => None,
        }
    }
}

pub fn init_message_reader(
    insim_sender: std::sync::mpsc::Sender<InsimCommand>,
) {
    std::thread::spawn(move || {
        let mut conn = insim::tcp(format!("{}:{}", INSIM_HOST, INSIM_PORT)).connect_blocking().unwrap();
        loop {
            match conn.read() {
                Ok(packet) => {
                    match packet {
                        insim::Packet::Mso(mso) => {
                            if let Some(cmd) = InsimCommand::from_string(mso.msg) {
                                insim_sender.send(cmd).unwrap();
                            }
                        }
                        _ => { /* Ignore other packets */ }
                    }
                },
                Err(e) => {
                    println!("Error reading from Insim connection: {:?}", e);
                    break;
                },
            }
        }
    });
}
