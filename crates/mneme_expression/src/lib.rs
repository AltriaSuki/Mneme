use std::time::Duration;

pub struct Humanizer {
    // Configuration placeholders
    read_speed_cpm: u32,
    typing_speed_cpm: u32,
}

impl Humanizer {
    pub fn new() -> Self {
        Self {
            read_speed_cpm: 1000, // conservative default
            typing_speed_cpm: 300,
        }
    }

    /// Calculate simulated delay for reading a message
    pub fn read_delay(&self, content: &str) -> Duration {
        let chars = content.chars().count() as u64;
        let ms_per_char = (60 * 1000) / self.read_speed_cpm as u64;
        // Base delay + variable delay based on length
        Duration::from_millis(500 + chars * ms_per_char)
    }

    /// Calculate simulated delay for typing a response
    pub fn typing_delay(&self, response: &str) -> Duration {
        let chars = response.chars().count() as u64;
        // Typing is slower than reading, usually. 
        // We add some overhead for "thinking" or "corrections".
        let ms_per_char = (60 * 1000) / self.typing_speed_cpm as u64;
        Duration::from_millis(1000 + chars * ms_per_char)
    }

    /// Split a long response into multiple messages
    pub fn split_response(&self, text: &str) -> Vec<String> {
        // TODO: Implement natural splitting logic
        vec![text.to_string()]
    }
}

impl Default for Humanizer {
    fn default() -> Self {
        Self::new()
    }
}
