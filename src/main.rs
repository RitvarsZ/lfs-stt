use futures::FutureExt;
use tracing::{info};
use tracing_subscriber::FmtSubscriber;

use crate::ui::UiContext;

mod insim_io;
mod ui;
mod audio;

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
    let subscriber = FmtSubscriber::builder()
    .with_max_level(tracing::Level::DEBUG)
    .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    let (mut audio_pipeline, mut stt_rx, audio_pipeline_handle) = audio::audio_pipeline::AudioPipeline::new().await?;
    let (insim, mut insim_rx, insim_handle) = insim_io::init_insim().await?;

    // UI context
    let mut ui_context = UiContext::default();

    let mut audio_pipeline_handle = audio_pipeline_handle.fuse();
    let mut insim_handle = insim_handle.fuse();

    loop {
        // Always dispatch UI events first
        ui_context.dispatch_ui_events(insim.clone()).await;

        tokio::select! {
            // Clear any UI message timeout
            _ = ui_context.clear_message_timeout() => {},

            // Process STT messages
            Some(msg) = stt_rx.recv() => {
                ui_context.handle_stt_message(msg);
            },

            // Process Insim events
            Some(event) = insim_rx.recv() => {
                ui_context.handle_insim_event(event, insim.clone(), &mut audio_pipeline).await;
            },

            _ = &mut insim_handle => {
                info!("Insim task has ended. Shutting down...");
                break;
            },
            _ = &mut audio_pipeline_handle => {
                info!("Audio pipeline task has ended. Shutting down...");
                break;
            },
        }
    }

    Ok(())
}

