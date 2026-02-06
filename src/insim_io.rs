use insim::builder::InsimTask;
use tokio::{sync::mpsc::Receiver, task::JoinHandle};
use tracing::{info};

use crate::global::CONFIG;

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

pub async fn init_insim() -> Result<(InsimTask, Receiver<InsimEvent>, JoinHandle<insim::Result<()>>), insim::Error> {
    info!("Connecting to INSIM at {}:{}", CONFIG.insim_host, CONFIG.insim_port);
    let (event_tx, event_rx) = tokio::sync::mpsc::channel(100);
    let (insim, handle) = loop {
        match insim::tcp(format!("{}:{}", CONFIG.insim_host, CONFIG.insim_port))
            .isi_iname("lfs-stt".to_owned())
            .isi_flag_local(true)
            .spawn(1)
            .await
        {
            Ok(v) => break v,
            Err(_) => {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    };
    info!("Connected to INSIM.");

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
