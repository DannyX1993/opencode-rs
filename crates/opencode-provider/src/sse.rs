//! Server-Sent Events (SSE) stream framing parser.
//!
//! Parses raw `text/event-stream` chunks into typed [`SseEvent`] values.
//! This is a pure function layer — no I/O, no async.

/// A single parsed SSE event.
#[derive(Debug, Clone, PartialEq)]
pub struct SseEvent {
    /// `event:` field value (defaults to `"message"` when absent).
    pub event: String,
    /// Accumulated `data:` field value (newlines joined).
    pub data: String,
    /// `id:` field value, if present.
    pub id: Option<String>,
    /// `retry:` field value in milliseconds, if present.
    pub retry: Option<u64>,
}

/// Stateful SSE decoder that handles partial-buffer continuation across chunks.
#[derive(Default)]
pub struct SseDecoder {
    buf: String,
}

impl SseDecoder {
    /// Create a new decoder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a chunk and return all fully delimited events found so far.
    ///
    /// Keeps any incomplete (no trailing blank line) data in the internal
    /// buffer for the next call.
    pub fn feed(&mut self, chunk: &str) -> Vec<SseEvent> {
        self.buf.push_str(chunk);
        let mut out = Vec::new();

        // Events are separated by blank lines ("\n\n" or "\r\n\r\n").
        loop {
            // Find the end of the next complete event block.
            let pos = if let Some(p) = self.buf.find("\n\n") {
                p
            } else if let Some(p) = self.buf.find("\r\n\r\n") {
                p
            } else {
                break;
            };

            let block_len = if self.buf[pos..].starts_with("\r\n\r\n") {
                pos + 4
            } else {
                pos + 2
            };

            let block = self.buf[..pos].to_string();
            self.buf = self.buf[block_len..].to_string();

            if let Some(ev) = parse_block(&block) {
                out.push(ev);
            }
        }

        out
    }
}

/// Parse a single event block (text between blank-line delimiters).
fn parse_block(block: &str) -> Option<SseEvent> {
    let mut event = "message".to_string();
    let mut data_parts: Vec<String> = Vec::new();
    let mut id: Option<String> = None;
    let mut retry: Option<u64> = None;

    for line in block.lines() {
        if line.starts_with(':') {
            // comment — skip
            continue;
        }
        if let Some(val) = line.strip_prefix("event:") {
            event = val.trim_start().to_string();
        } else if let Some(val) = line.strip_prefix("data:") {
            data_parts.push(val.trim_start().to_string());
        } else if let Some(val) = line.strip_prefix("id:") {
            id = Some(val.trim_start().to_string());
        } else if let Some(val) = line.strip_prefix("retry:") {
            retry = val.trim_start().parse().ok();
        }
    }

    // An event with no data lines is a flush — skip silently (no output).
    if data_parts.is_empty() {
        return None;
    }

    Some(SseEvent {
        event,
        data: data_parts.join("\n"),
        id,
        retry,
    })
}

/// Parse a complete SSE chunk into events. Stateless convenience wrapper.
///
/// Use [`SseDecoder`] when you need to handle partial-buffer continuation.
pub fn parse_events(chunk: &str) -> Vec<SseEvent> {
    SseDecoder::new().feed(chunk)
}

#[cfg(test)]
mod tests {
    use super::*;

    // RED 2.1 — data: line
    #[test]
    fn parses_data_line() {
        let chunk = "data: hello world\n\n";
        let events = parse_events(chunk);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "hello world");
        assert_eq!(events[0].event, "message");
    }

    // RED 2.1 — event: line overrides default name
    #[test]
    fn parses_event_name() {
        let chunk = "event: content_block_delta\ndata: {\"text\":\"hi\"}\n\n";
        let events = parse_events(chunk);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, "content_block_delta");
        assert_eq!(events[0].data, "{\"text\":\"hi\"}");
    }

    // RED 2.1 — empty-line flush (no data) produces no event
    #[test]
    fn empty_line_flush_produces_no_event() {
        let chunk = "\n\n";
        let events = parse_events(chunk);
        assert_eq!(events.len(), 0);
    }

    // RED 2.1 — retry: line
    #[test]
    fn parses_retry_line() {
        let chunk = "retry: 3000\ndata: ping\n\n";
        let events = parse_events(chunk);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].retry, Some(3000));
        assert_eq!(events[0].data, "ping");
    }

    // RED 2.1 — partial-buffer continuation via SseDecoder
    #[test]
    fn partial_buffer_continuation() {
        let mut dec = SseDecoder::new();
        // First chunk: incomplete, no blank line
        let partial = dec.feed("data: partial");
        assert!(partial.is_empty());
        // Second chunk: completes the event
        let events = dec.feed(" value\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "partial value");
    }

    // TRIANGULATE: multiple events in one chunk
    #[test]
    fn multiple_events_in_one_chunk() {
        let chunk = "data: first\n\ndata: second\n\n";
        let events = parse_events(chunk);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].data, "first");
        assert_eq!(events[1].data, "second");
    }

    // TRIANGULATE: multi-line data is joined with \n
    #[test]
    fn multiline_data_joined() {
        let chunk = "data: line1\ndata: line2\n\n";
        let events = parse_events(chunk);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "line1\nline2");
    }

    // TRIANGULATE: id: line is captured
    #[test]
    fn parses_id_field() {
        let chunk = "id: 42\ndata: payload\n\n";
        let events = parse_events(chunk);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, Some("42".into()));
    }

    // TRIANGULATE: comment lines are ignored
    #[test]
    fn comment_lines_ignored() {
        let chunk = ": this is a comment\ndata: real\n\n";
        let events = parse_events(chunk);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "real");
    }
}
