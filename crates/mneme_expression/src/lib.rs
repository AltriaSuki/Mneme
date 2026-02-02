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
        let mut parts = Vec::new();
        let mut current_part = String::new();
        
        // Simple heuristic: 
        // 1. Split by double newlines (paragraphs)
        // 2. If a part is still too long (> 100 chars), try to split by sentence endings
        //    (., !, ?, 。, ！, ？)
        
        for line in text.split('\n') {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            
            // If current_part + line is acceptable, append.
            // Otherwise push current_part and start new.
            // "Acceptable" hard limit is arbitrary for now, say 200 chars.
            if !current_part.is_empty() && current_part.len() + line.len() > 100 { // prefer short messages
                 parts.push(current_part);
                 current_part = String::from(line);
            } else {
                if !current_part.is_empty() {
                    current_part.push('\n');
                }
                current_part.push_str(line);
            }
        }
        
        if !current_part.is_empty() {
            parts.push(current_part);
        }
        
        // Refinement: check for internal sentence splits if parts are massive?
        // For now, let's stick to the paragraph/line merging strategy which is robust enough for LLM output.
        // LLMs usually output paragraphs.
        
        parts
    }
}

impl Default for Humanizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_response() {
        let humanizer = Humanizer::new();
        // Create text > 100 chars to force split
        let p1 = "This is the first paragraph. It is reasonably long but not too long."; // ~60
        let p2 = "This is the second paragraph. It is also quite long and when combined with the first one it should definitely exceed the limit of one hundred characters set in the code."; // ~160
        let text = format!("{}\n\n{}", p1, p2);
        
        let parts = humanizer.split_response(&text);
        assert!(parts.len() >= 2, "Expected at least 2 parts, got {}", parts.len());
    }
    
    #[test]
    fn test_delays() {
        let humanizer = Humanizer::new();
        let short = humanizer.read_delay("short");
        let long = humanizer.read_delay("this is a much longer message that should take longer to read");
        assert!(long > short);
    }
}
