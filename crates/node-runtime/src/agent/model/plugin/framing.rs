//! Network framing remains native; package code receives complete UTF-8 events.
use sailry_protocol::{
    Fault,
    plugin::model_adapter::{Framing, wire::Event},
};

pub(super) struct Decoder {
    framing: Framing,
    pending: Vec<u8>,
    data: Vec<String>,
    event: Option<String>,
    bytes: usize,
    limit: usize,
    first: bool,
}

impl Decoder {
    pub(super) fn new(framing: Framing, limit: usize) -> Self {
        Self {
            framing,
            pending: Vec::new(),
            data: Vec::new(),
            event: None,
            bytes: 0,
            limit,
            first: true,
        }
    }

    pub(super) fn push(&mut self, bytes: &[u8], done: bool) -> Result<Vec<Event>, Fault> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .filter(|bytes| *bytes <= self.limit)
            .ok_or_else(|| {
                Fault::new(
                    sailry_protocol::ErrorCode::Internal,
                    "model adapter resource budget exceeded",
                )
            })?;
        self.pending.extend_from_slice(bytes);
        if self.framing == Framing::Json {
            return if done {
                Ok(vec![Event {
                    event: None,
                    data: self.text()?,
                }])
            } else {
                Ok(Vec::new())
            };
        }
        let mut events = Vec::new();
        while let Some(end) = self
            .pending
            .iter()
            .position(|byte| matches!(byte, b'\n' | b'\r'))
        {
            if self.pending[end] == b'\r' && end + 1 == self.pending.len() && !done {
                break;
            }
            let terminator =
                if self.pending[end] == b'\r' && self.pending.get(end + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
            let line: Vec<_> = self.pending.drain(..end + terminator).collect();
            self.line(&line[..end], &mut events)?;
        }
        if done {
            if !self.pending.is_empty() {
                let line = std::mem::take(&mut self.pending);
                self.line(&line, &mut events)?;
            }
            self.flush(&mut events);
        }
        Ok(events)
    }

    fn text(&mut self) -> Result<String, Fault> {
        String::from_utf8(std::mem::take(&mut self.pending))
            .map_err(|_| Fault::invalid("model response is not UTF-8"))
    }

    fn line(&mut self, line: &[u8], events: &mut Vec<Event>) -> Result<(), Fault> {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let line =
            std::str::from_utf8(line).map_err(|_| Fault::invalid("model response is not UTF-8"))?;
        let line = if std::mem::take(&mut self.first) {
            line.strip_prefix('\u{feff}').unwrap_or(line)
        } else {
            line
        };
        if self.framing == Framing::Ndjson {
            if !line.trim().is_empty() {
                events.push(Event {
                    event: None,
                    data: line.into(),
                });
            }
        } else if line.is_empty() {
            self.flush(events);
        } else if !line.starts_with(':') {
            let (field, value) = line.split_once(':').unwrap_or((line, ""));
            let value = value.strip_prefix(' ').unwrap_or(value);
            match field {
                "data" => self.data.push(value.into()),
                "event" => self.event = Some(value.into()),
                _ => (),
            }
        }
        Ok(())
    }

    fn flush(&mut self, events: &mut Vec<Event>) {
        if !self.data.is_empty() {
            events.push(Event {
                event: self.event.take(),
                data: self.data.join("\n"),
            });
            self.data.clear();
        } else {
            self.event = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Decoder, Framing};

    #[test]
    fn preserves_split_utf8_and_multiline_data() {
        let mut decoder = Decoder::new(Framing::Sse, 1024);
        let bytes = "event: delta\r\ndata: 中文\r\ndata: second\r\n\r\n".as_bytes();
        let split = bytes.iter().position(|byte| *byte == 0xe4).unwrap() + 1;
        assert!(decoder.push(&bytes[..split], false).unwrap().is_empty());
        let events = decoder.push(&bytes[split..], true).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event.as_deref(), Some("delta"));
        assert_eq!(events[0].data, "中文\nsecond");
    }

    #[test]
    fn keeps_final_line_and_response_budget() {
        let mut decoder = Decoder::new(Framing::Ndjson, 8);
        assert_eq!(decoder.push(b"{}\n{}", true).unwrap().len(), 2);
        assert!(decoder.push(b"1234", false).is_err());
        let mut decoder = Decoder::new(Framing::Json, 8);
        assert!(decoder.push(b"{}", false).unwrap().is_empty());
        assert_eq!(decoder.push(b"", true).unwrap()[0].data, "{}");
    }
}
