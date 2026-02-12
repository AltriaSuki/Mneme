//! Text-to-Speech (TTS) trait definition

use anyhow::Result;
use async_trait::async_trait;
use mneme_core::Emotion;

/// Output format for synthesized audio
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    #[default]
    Mp3,
    Wav,
    OggOpus,
    Pcm,
}

/// Text-to-Speech trait for synthesizing audio from text
#[async_trait]
pub trait TextToSpeech: Send + Sync {
    /// Synthesize text to audio
    ///
    /// # Arguments
    /// * `text` - Text to synthesize
    /// * `emotion` - Optional emotional tone
    ///
    /// # Returns
    /// Raw audio bytes in the default format (usually MP3)
    async fn synthesize(&self, text: &str, emotion: Option<Emotion>) -> Result<Vec<u8>>;

    /// Synthesize with a specific output format
    ///
    /// Default implementation returns an error. Implementations that support
    /// multiple formats should override this method.
    async fn synthesize_with_format(
        &self,
        _text: &str,
        _emotion: Option<Emotion>,
        format: OutputFormat,
    ) -> Result<Vec<u8>> {
        anyhow::bail!(
            "Output format {:?} not supported by {}. Use synthesize() for default format.",
            format,
            self.provider_name()
        )
    }

    /// Get the default output format for this provider
    fn default_format(&self) -> OutputFormat {
        OutputFormat::Mp3
    }

    /// Get the voice identifier being used
    fn voice_id(&self) -> &str;

    /// Get the name of this TTS provider
    fn provider_name(&self) -> &'static str;

    /// Check if this TTS engine supports emotional synthesis
    fn supports_emotion(&self) -> bool {
        false
    }

    /// Check if this TTS engine supports a specific output format
    fn supports_format(&self, format: OutputFormat) -> bool {
        format == self.default_format()
    }
}
