use tokio::task::JoinError;

mod recorder;
mod resampler;
pub mod speech_to_text;
pub mod audio_pipeline;

#[derive(Debug, thiserror::Error)]
pub enum AudioPipelineError {
    #[error("audio device error")]
    AudioDevice(#[from] AudioBackendError),

    #[error("resampling error")]
    Resampler(#[from] ResamplerError),

    #[error("speech-to-text error")]
    SpeechToText(#[from] whisper_rs::WhisperError),

    #[error("model file not found, check you config")]
    ModelNotFound,

    #[error("audio debug error")]
    AudioDebugError(String),

    #[error("audio pipeline task error")]
    AudioPipelineTaskJoinError(JoinError)
}

#[derive(Debug, thiserror::Error)]
pub enum AudioBackendError {
    #[error("failed to build audio stream")]
    BuildStream(#[from] cpal::BuildStreamError),

    #[error("no audio input device available")]
    NoInputDevice,

    #[error("unsupported number of input channels. Only mono and stereo input devices are supported.")]
    UnsupportedInputChannels,

    #[error("failed to play audio stream")]
    PlayStream(#[from] cpal::PlayStreamError),

    #[error("failed to pause audio stream")]
    PauseStream(#[from] cpal::PauseStreamError),

    #[error("failed to enumerate audio devices")]
    Devices(#[from] cpal::DevicesError),

    #[error("failed to get default stream config")]
    DefaultConfig(#[from] cpal::DefaultStreamConfigError),
}

#[derive(Debug, thiserror::Error)]
pub enum ResamplerError {
    #[error("failed to resample")]
    Resample(#[from] rubato::ResampleError),

    #[error("failed to initialize resampler")]
    ResamplerConstructionError(#[from] rubato::ResamplerConstructionError),
}
