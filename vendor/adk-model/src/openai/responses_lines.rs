//! Incremental SSE line framing across arbitrary HTTP byte boundaries.

#[derive(Default)]
pub(super) struct Lines {
    pending: Vec<u8>,
    skip_lf: bool,
}

impl Lines {
    pub(super) fn push(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut lines = Vec::new();
        for &byte in bytes {
            if self.skip_lf {
                self.skip_lf = false;
                if byte == b'\n' {
                    continue;
                }
            }
            if matches!(byte, b'\r' | b'\n') {
                self.skip_lf = byte == b'\r';
                self.pending.push(b'\n');
                lines.push(std::mem::take(&mut self.pending));
            } else {
                self.pending.push(byte);
            }
        }
        lines
    }

    pub(super) fn finish(self) -> Option<Vec<u8>> {
        (!self.pending.is_empty()).then_some(self.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_fragmented_events() {
        for ending in ["\n", "\r\n", "\r"] {
            let body = format!(
                "event: response.output_text.delta{ending}data: {{\"delta\":\"🙂\"}}{ending}{ending}"
            );
            for width in 1..=body.len() {
                let mut lines = Lines::default();
                let output = body
                    .as_bytes()
                    .chunks(width)
                    .flat_map(|chunk| lines.push(chunk))
                    .flatten()
                    .collect::<Vec<_>>();
                assert_eq!(
                    output,
                    "event: response.output_text.delta\ndata: {\"delta\":\"🙂\"}\n\n".as_bytes()
                );
                assert!(lines.finish().is_none());
            }
        }
    }

    #[test]
    fn flushes_cr_without_waiting() {
        let mut lines = Lines::default();
        assert_eq!(
            lines.push(b"data: {}\r\r"),
            [b"data: {}\n".to_vec(), b"\n".to_vec()]
        );
        assert!(lines.push(b"\n").is_empty());
        assert_eq!(lines.push(b"data: next\r"), [b"data: next\n".to_vec()]);
        assert!(lines.push(b"\n").is_empty());
        assert!(lines.finish().is_none());
    }

    #[test]
    fn retains_unterminated_tail() {
        let mut lines = Lines::default();
        assert_eq!(
            lines.push(b": heartbeat\n\r\ndata: tail"),
            [b": heartbeat\n".to_vec(), b"\n".to_vec()]
        );
        assert_eq!(lines.finish(), Some(b"data: tail".to_vec()));
    }
}
