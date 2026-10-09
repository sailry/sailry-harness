use bytes::{Buf, BytesMut};
use serde_json::Value;
use std::io;
use tokio_util::codec::{Decoder, Encoder};

const HEADERS: usize = 8192;
pub(super) const BODY: usize = 8 * 1024 * 1024;

#[derive(Default)]
pub(super) struct Codec {
    length: Option<usize>,
}

impl Decoder for Codec {
    type Item = Value;
    type Error = io::Error;

    fn decode(&mut self, source: &mut BytesMut) -> Result<Option<Value>, io::Error> {
        if self.length.is_none() {
            let Some(end) = source.windows(4).position(|value| value == b"\r\n\r\n") else {
                if source.len() > HEADERS {
                    return Err(invalid("language response headers exceed the limit"));
                }
                return Ok(None);
            };
            if end > HEADERS {
                return Err(invalid("language response headers exceed the limit"));
            }
            let headers = std::str::from_utf8(&source[..end])
                .map_err(|_| invalid("language response headers are not UTF-8"))?;
            let mut length = None;
            for line in headers.split("\r\n") {
                let (name, value) = line
                    .split_once(':')
                    .ok_or_else(|| invalid("invalid language response header"))?;
                if name.eq_ignore_ascii_case("Content-Length") {
                    if length.is_some() {
                        return Err(invalid("duplicate language response length"));
                    }
                    let size = value
                        .trim()
                        .parse::<usize>()
                        .map_err(|_| invalid("invalid language response length"))?;
                    if size == 0 || size > BODY {
                        return Err(invalid("language response exceeds the limit"));
                    }
                    length = Some(size);
                }
            }
            self.length = Some(length.ok_or_else(|| invalid("language response has no length"))?);
            source.advance(end + 4);
        }
        let length = self.length.expect("checked frame length");
        if source.len() < length {
            return Ok(None);
        }
        let message = serde_json::from_slice(&source.split_to(length))
            .map_err(|_| invalid("invalid language response JSON"))?;
        self.length = None;
        Ok(Some(message))
    }
}

impl Encoder<Value> for Codec {
    type Error = io::Error;
    fn encode(&mut self, message: Value, destination: &mut BytesMut) -> Result<(), io::Error> {
        let body = serde_json::to_vec(&message).map_err(io::Error::other)?;
        if body.len() > BODY {
            return Err(invalid("language request exceeds the limit"));
        }
        destination.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
        destination.extend_from_slice(&body);
        Ok(())
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests;
