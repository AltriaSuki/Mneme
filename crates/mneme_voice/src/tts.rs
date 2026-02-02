//! Text-to-Speech (TTS) trait definition

use anyhow::Result;
use async_trait::async_trait;

/// Emotional tone for TTS synthesis
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Emotion {
    #[default]
    Neutral,
    Happy,
    Sad,
    Excited,
    Calm,
    Angry,
    Surprised,
}

impl Emotion {
    /// Get a descriptive name for the emotion
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Neutral => "neutral",
            Self::Happy => "happy",
            Self::Sad => "sad",
            Self::Excited => "excited",
            Self::Calm => "calm",
            Self::Angry => "angry",
            Self::Surprised => "surprised",
        }
    }
}

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
    async fn synthesize_with_format(
        &self,
        text: &str,
        emotion: Option<Emotion>,
        format: OutputFormat,
    ) -> Result<Vec<u8>> {
        // Default implementation ignores format and uses synthesize
        // Concrete implementations can override for format support
        let _ = format;
        self.synthesize(text, emotion).await
    }
    
    /// Get the voice identifier being used
    fn voice_id(&self) -> &str;
    
    /// Get the name of this TTS provider
    fn provider_name(&self) -> &'static str;
    
    /// Check if this TTS engine supports emotional synthesis
    fn supports_emotion(&self) -> bool {
        false
    }
}
