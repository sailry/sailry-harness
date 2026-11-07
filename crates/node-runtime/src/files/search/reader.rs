use super::{Control, MAX_BYTES};
use std::io::{self, Read};

pub(super) struct Text<'a, R> {
    source: R,
    control: &'a Control,
    pending: Vec<u8>,
    total: usize,
    pub binary: bool,
}

impl<'a, R: Read> Text<'a, R> {
    pub fn new(source: R, control: &'a Control) -> Self {
        Self {
            source,
            control,
            pending: Vec::with_capacity(4),
            total: 0,
            binary: false,
        }
    }

    fn validate(&mut self, mut bytes: &[u8]) -> io::Result<()> {
        if bytes.contains(&0) {
            self.binary = true;
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "binary file cannot be searched as text",
            ));
        }
        if !self.pending.is_empty() {
            let width = if self.pending[0] < 0xe0 {
                2
            } else if self.pending[0] < 0xf0 {
                3
            } else {
                4
            };
            let count = bytes.len().min(width - self.pending.len());
            self.pending.extend_from_slice(&bytes[..count]);
            bytes = &bytes[count..];
            match std::str::from_utf8(&self.pending) {
                Ok(_) => self.pending.clear(),
                Err(error) if error.error_len().is_none() => return Ok(()),
                Err(_) => return Err(invalid()),
            }
        }
        match std::str::from_utf8(bytes) {
            Ok(_) => Ok(()),
            Err(error) if error.error_len().is_none() => {
                self.pending
                    .extend_from_slice(&bytes[error.valid_up_to()..]);
                Ok(())
            }
            Err(_) => Err(invalid()),
        }
    }
}

impl<R: Read> Read for Text<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        self.control
            .check()
            .map_err(|error| io::Error::other(error.message))?;
        let capacity = buffer.len().min(8192);
        let count = self.source.read(&mut buffer[..capacity])?;
        if count == 0 && !self.pending.is_empty() {
            return Err(invalid());
        }
        self.total = self.total.saturating_add(count);
        if self.total > MAX_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "file exceeds the 64 MiB search limit",
            ));
        }
        self.validate(&buffer[..count])?;
        Ok(count)
    }
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "file is not UTF-8 text")
}
