use futures::FutureExt;
use tracing::{warn, info, error, level_filters::LevelFilter};
use tracing_subscriber::FmtSubscriber;

use crate::{global::CONFIG, ui::UiContext};

mod insim_io;
mod ui;
mod audio;
mod config;
mod global;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(LevelFilter::from(CONFIG.debug_log_level))
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    let (insim, mut insim_rx, insim_handle) = insim_io::init_insim().await?;
    let (mut audio_pipeline, mut stt_rx, audio_pipeline_handle) = audio::audio_pipeline::AudioPipeline::new().await?;

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

            res = &mut insim_handle => {
                match res {
                    Ok(Ok(())) => info!("Insim task ended successfully."),
                    Ok(Err(e)) => warn!("{}", e),
                    Err(e) => error!("{}", e),
                }
                break;
            },
            _ = &mut audio_pipeline_handle => {
                info!("Audio pipeline task has ended.");
                break;
            },
        }
    }

    Ok(())
}

