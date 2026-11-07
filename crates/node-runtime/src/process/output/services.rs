//! Announced loopback URLs are candidates, not a scan of listening ports.
use sailry_protocol::process::Service;
#[derive(Default)]
pub(crate) struct Scanner {
    escape: u8,
    prefix: Vec<u8>,
    candidate: Option<Vec<u8>>,
    oversized: bool,
    pending: Vec<Service>,
    found: Vec<Service>,
    unicode: [u8; 4],
    count: usize,
    pending_bytes: Vec<u8>,
    offset: usize,
}

impl Scanner {
    pub(super) fn decode(&mut self, bytes: &[u8], decoder: &super::Decoder, complete: bool) {
        if self.found.len() >= 16 {
            return;
        }
        self.pending_bytes.extend_from_slice(bytes);
        let range = decoder.bounds(&self.pending_bytes, self.offset, complete);
        let (text, _) = decoder.decode(
            &self.pending_bytes[range.clone()],
            self.offset + range.start,
        );
        self.append(text.as_bytes());
        self.pending_bytes.drain(..range.end);
        self.offset += range.end;
    }

    pub(crate) fn append(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if self.found.len() >= 16 {
                break;
            }
            match (self.escape, byte) {
                (0, 0x1b) => self.escape = 1,
                (1, b'[') => self.escape = 2,
                (2, 0x40..=0x7e) => self.escape = 0,
                (2, 0x20..=0x3f) => {}
                (0, _) => self.text(byte),
                _ => {
                    self.escape = 0;
                    self.text(0x1b);
                    self.text(byte);
                }
            }
        }
    }

    fn text(&mut self, byte: u8) {
        if byte.is_ascii() {
            for index in 0..self.count {
                self.plain(self.unicode[index]);
            }
            self.count = 0;
            self.plain(byte);
            return;
        }
        self.unicode[self.count] = byte;
        self.count += 1;
        match std::str::from_utf8(&self.unicode[..self.count]) {
            Ok(text) if text.chars().all(char::is_whitespace) => self.plain(b' '),
            Err(error) if error.error_len().is_none() => return,
            _ => {
                for index in 0..self.count {
                    self.plain(self.unicode[index]);
                }
            }
        }
        self.count = 0;
    }

    fn plain(&mut self, byte: u8) {
        if byte.is_ascii_whitespace()
            || matches!(byte, 0..=0x1f | b'<' | b'>' | b'"' | b'\'' | b'`')
        {
            if let Some(candidate) = self.candidate.take()
                && !self.oversized
                && let Some(service) = parse(&candidate)
                && self.found.len() + self.pending.len() < 16
                && !self
                    .found
                    .iter()
                    .chain(&self.pending)
                    .any(|found| found.port == service.port)
            {
                self.pending.push(service);
            }
            self.prefix.clear();
            self.oversized = false;
            if matches!(byte, b'\n' | b'\r') {
                self.found.append(&mut self.pending);
            }
            return;
        }
        if let Some(candidate) = &mut self.candidate {
            if candidate.len() < 2048 {
                candidate.push(byte);
            } else {
                self.oversized = true;
            }
            return;
        }
        self.prefix.push(byte);
        while !b"http://".starts_with(&self.prefix) && !b"https://".starts_with(&self.prefix) {
            self.prefix.remove(0);
        }
        if self.prefix.as_slice() == b"http://" || self.prefix.as_slice() == b"https://" {
            self.candidate = Some(std::mem::take(&mut self.prefix));
        }
    }

    pub(crate) fn items(&self) -> &[Service] {
        &self.found
    }
}

fn parse(candidate: &[u8]) -> Option<Service> {
    let text = String::from_utf8_lossy(candidate);
    let value = text.trim_end_matches([')', ']', ',', '.', ';']);
    let mut url = url::Url::parse(value).ok()?;
    if !matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "0.0.0.0"))
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    let port = url.port_or_known_default().filter(|port| *port != 0)?;
    url.set_host(Some("127.0.0.1")).unwrap();
    Some(Service {
        port,
        url: url.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_for_complete_lines_and_bounds_candidates() {
        let mut scanner = Scanner::default();
        scanner.append(b"http://localhost:51");
        assert!(scanner.items().is_empty());
        scanner.append(b"73/app");
        assert!(scanner.items().is_empty());
        scanner.append(b"\n");
        assert_eq!(scanner.items()[0].port, 5173);
        scanner.append(format!("http://localhost:7000/{}\n", "x".repeat(2048)).as_bytes());
        assert_eq!(scanner.items().len(), 1);
        for port in 8000..8030 {
            scanner.append(format!("http://localhost:{port}/\n").as_bytes());
        }
        assert_eq!(scanner.items().len(), 16);
    }

    #[test]
    fn keeps_paths_and_ignores_unrelated_addresses() {
        let mut scanner = Scanner::default();
        scanner.append(
            b"Local: \x1b[36mhttp://localhost:5173/app?q=1\x1b[0m\nhttp://0.0.0.0:5173/ http://127.0.0.1:8000/ http://example.com:1234/ http://localhost.evil:99/ http://user:secret@localhost:30/ http://localhost:0/\n",
        );
        assert_eq!(
            scanner.items(),
            [
                Service {
                    port: 5173,
                    url: "http://127.0.0.1:5173/app?q=1".into()
                },
                Service {
                    port: 8000,
                    url: "http://127.0.0.1:8000/".into()
                },
            ]
        );
        scanner.append(b"http://localhost:5173/\n");
        assert_eq!(scanner.items().len(), 2);
    }

    #[test]
    fn preserves_split_urls_and_escapes() {
        let text = "Local: \x1b[36mhttps://local\x1b[0mhost:5173/项目?q=1\x1b[0m\n";
        for split in 0..text.len() {
            let mut scanner = Scanner::default();
            scanner.append(&text.as_bytes()[..split]);
            scanner.append(&text.as_bytes()[split..]);
            assert_eq!(scanner.items().len(), 1, "{split}");
            assert_eq!(
                scanner.items()[0].url,
                "https://127.0.0.1:5173/%E9%A1%B9%E7%9B%AE?q=1"
            );
        }
        let mut scanner = Scanner::default();
        for byte in text.as_bytes() {
            scanner.append(std::slice::from_ref(byte));
        }
        assert_eq!(scanner.items().len(), 1);
    }

    #[test]
    fn separates_unicode_whitespace_across_chunks() {
        let text = "http://localhost:5173/　http://localhost:8000/\n";
        let mut scanner = Scanner::default();
        for byte in text.as_bytes() {
            scanner.append(std::slice::from_ref(byte));
        }
        assert_eq!(
            scanner
                .items()
                .iter()
                .map(|service| service.port)
                .collect::<Vec<_>>(),
            vec![5173, 8000]
        );
    }
}
