//! Speech-to-Text (STT) trait definition

use anyhow::Result;
use async_trait::async_trait;

/// Supported audio formats for STT
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    /// WAV format
    Wav,
    /// MP3 format
    Mp3,
    /// OGG Opus (common for voice messages)
    OggOpus,
    /// Silk format (used by QQ/WeChat)
    /// Note: No standard IANA MIME type; common variants are audio/silk, audio/x-silk
    Silk,
    /// Raw PCM
    Pcm { sample_rate: u32, channels: u8 },
}

impl AudioFormat {
    /// Get the MIME type for this format
    pub fn mime_type(&self) -> &'static str {
        match self {
            Self::Wav => "audio/wav",
            Self::Mp3 => "audio/mpeg",
            Self::OggOpus => "audio/ogg",
            Self::Silk => "audio/silk",
            Self::Pcm { .. } => "audio/pcm",
        }
    }
}

/// Speech-to-Text trait for transcribing audio to text
#[async_trait]
pub trait SpeechToText: Send + Sync {
    /// Transcribe audio data to text
    ///
    /// # Arguments
    /// * `audio` - Raw audio bytes
    /// * `format` - Audio format of the input
    ///
    /// # Returns
    /// Transcribed text
    async fn transcribe(&self, audio: &[u8], format: AudioFormat) -> Result<String>;

    /// Check if this STT engine supports a given language
    fn supports_language(&self, lang: &str) -> bool;

    /// Get the name of this STT provider
    fn provider_name(&self) -> &'static str;
}
