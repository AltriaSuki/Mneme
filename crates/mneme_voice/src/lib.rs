//! Voice module for Mneme
//!
//! Provides Speech-to-Text (STT) and Text-to-Speech (TTS) abstractions.
//! Concrete implementations are added via feature flags or external crates.

mod stt;
mod tts;

pub use stt::{AudioFormat, SpeechToText};
pub use tts::{Emotion, TextToSpeech};
