use insim::builder::InsimTask;
use tokio::{sync::mpsc::Receiver, task::JoinHandle};
use tracing::{error, info};

use crate::{INSIM_HOST, INSIM_PORT};

#[allow(dead_code)]
pub enum InsimEvent {
    ToggleRecording,
    AcceptMessage,
    NextChannel,
    PeviousChannel,
    IsInGame(bool),
}

impl InsimEvent {
    pub fn from_string(cmd: String) -> Option<InsimEvent> {
        match cmd.as_str() {
            "stt talk" => Some(InsimEvent::ToggleRecording),
            "stt accept" => Some(InsimEvent::AcceptMessage),
            "stt nc" => Some(InsimEvent::NextChannel),
            "stt pc" => Some(InsimEvent::PeviousChannel),
            _ => None,
        }
    }
}

pub async fn init_insim() -> Result<(InsimTask, Receiver<InsimEvent>, JoinHandle<insim::Result<()>>), Box<dyn std::error::Error>> {
    info!("Connecting to INSIM at {}:{}", INSIM_HOST, INSIM_PORT);
    let (event_tx, event_rx) = tokio::sync::mpsc::channel(100);
    let (insim, handle) = match insim::tcp(format!("{}:{}", INSIM_HOST, INSIM_PORT))
        .isi_iname("lfs-stt".to_owned())
        .isi_flag_local(true)
        .spawn(1).await {
        Ok(c) => c,
        Err(err) => {
            error!("Failed to connect to INSIM: {}", err);
            return Err(Box::new(err));
        },
    };

    let mut rx = insim.subscribe();
    tokio::spawn(async move {
        loop {
            while let Ok(packet) = rx.recv().await {
                match packet {
                    insim::Packet::Mso(mso) => {
                        if let Some(cmd) = InsimEvent::from_string(mso.msg) {
                            let _ = event_tx.send(cmd).await;
                        }
                    },
                    insim::Packet::Sta(sta) => {
                        let _ = event_tx.send(InsimEvent::IsInGame(sta.flags.is_in_game())).await;
                    }
                    _ => {}
                };
            }
        }
    });

    // Request initial game state info.
    insim.send(insim::Packet::Tiny(insim::insim::Tiny{
        subt: insim::insim::TinyType::Sst,
        reqi: insim::identifiers::RequestId::from(1),
    })).await?;

    Ok((insim, event_rx, handle))
}
