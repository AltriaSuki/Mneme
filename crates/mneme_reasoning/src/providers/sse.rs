//! Shared SSE buffer management for streaming providers.
//!
//! Both Anthropic and OpenAI SSE parsers need the same buffer lifecycle:
//! push raw bytes → extract complete lines or event blocks → handle residue.

/// A reusable buffer for SSE byte stream parsing.
pub(crate) struct SseBuffer {
    buffer: String,
}

impl SseBuffer {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
        }
    }

    /// Append raw bytes (lossy UTF-8) to the buffer.
    pub fn push_bytes(&mut self, chunk: &bytes::Bytes) {
        self.buffer.push_str(&String::from_utf8_lossy(chunk));
    }

    /// Extract complete newline-terminated lines from the buffer (OpenAI format).
    ///
    /// Returns all lines that end with `\n`. Partial trailing data stays in the buffer.
    pub fn extract_lines(&mut self) -> Vec<String> {
        let mut lines = Vec::new();
        while let Some(pos) = self.buffer.find('\n') {
            let line = self.buffer[..pos].trim().to_string();
            self.buffer = self.buffer[pos + 1..].to_string();
            lines.push(line);
        }
        lines
    }

    /// Extract complete `\n\n`-delimited event blocks from the buffer (Anthropic format).
    ///
    /// Returns all blocks separated by double newlines. Partial trailing data stays in the buffer.
    pub fn extract_event_blocks(&mut self) -> Vec<String> {
        let mut blocks = Vec::new();
        while let Some(pos) = self.buffer.find("\n\n") {
            let block = self.buffer[..pos].to_string();
            self.buffer = self.buffer[pos + 2..].to_string();
            blocks.push(block);
        }
        blocks
    }

    /// Return the remaining (incomplete) data in the buffer.
    pub fn residue(&self) -> &str {
        &self.buffer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_lines_complete() {
        let mut buf = SseBuffer::new();
        buf.push_bytes(&bytes::Bytes::from("data: hello\ndata: world\n"));
        let lines = buf.extract_lines();
        assert_eq!(lines, vec!["data: hello", "data: world"]);
        assert!(buf.residue().is_empty());
    }

    #[test]
    fn test_extract_lines_partial() {
        let mut buf = SseBuffer::new();
        buf.push_bytes(&bytes::Bytes::from("data: hello\ndata: wor"));
        let lines = buf.extract_lines();
        assert_eq!(lines, vec!["data: hello"]);
        assert_eq!(buf.residue(), "data: wor");

        // Push the rest
        buf.push_bytes(&bytes::Bytes::from("ld\n"));
        let lines = buf.extract_lines();
        assert_eq!(lines, vec!["data: world"]);
        assert!(buf.residue().is_empty());
    }

    #[test]
    fn test_extract_event_blocks() {
        let mut buf = SseBuffer::new();
        buf.push_bytes(&bytes::Bytes::from(
            "event: delta\ndata: {\"x\":1}\n\nevent: stop\ndata: {}\n\n",
        ));
        let blocks = buf.extract_event_blocks();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0], "event: delta\ndata: {\"x\":1}");
        assert_eq!(blocks[1], "event: stop\ndata: {}");
        assert!(buf.residue().is_empty());
    }

    #[test]
    fn test_extract_event_blocks_with_residue() {
        let mut buf = SseBuffer::new();
        buf.push_bytes(&bytes::Bytes::from(
            "event: delta\ndata: {\"x\":1}\n\nevent: partial",
        ));
        let blocks = buf.extract_event_blocks();
        assert_eq!(blocks.len(), 1);
        assert_eq!(buf.residue(), "event: partial");
    }
}
