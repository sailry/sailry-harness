//! Assistant Markdown can show an image already returned in its own conversation.
use std::fmt;

pub const SCHEME: &str = "sailry-image";
const MAX_CALL_BYTES: usize = 512;

/// `sailry-image:<tool-call-id>/<index>` names one tool result image. The index
/// is the image's original position in that result, as in [`super::Image::index`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Reference {
    pub call: String,
    pub index: usize,
}

impl Reference {
    pub fn parse(value: &str) -> Option<Self> {
        let (scheme, rest) = value.split_once(':')?;
        if !scheme.eq_ignore_ascii_case(SCHEME) {
            return None;
        }
        let (call, index) = rest.rsplit_once('/')?;
        if index.is_empty()
            || !index.bytes().all(|byte| byte.is_ascii_digit())
            || (index.len() > 1 && index.starts_with('0'))
        {
            return None;
        }
        let call = decode(call)?;
        (!call.is_empty() && call.len() <= MAX_CALL_BYTES).then_some(Self {
            call,
            index: index.parse().ok()?,
        })
    }
}

impl fmt::Display for Reference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{SCHEME}:")?;
        for byte in self.call.bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                write!(formatter, "{}", byte as char)?;
            } else {
                write!(formatter, "%{byte:02X}")?;
            }
        }
        write!(formatter, "/{}", self.index)
    }
}

fn decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut position = 0;
    while position < bytes.len() {
        if bytes[position] == b'%' {
            let digits = bytes.get(position + 1..position + 3)?;
            if !digits.iter().all(u8::is_ascii_hexdigit) {
                return None;
            }
            output.push(u8::from_str_radix(std::str::from_utf8(digits).ok()?, 16).ok()?);
            position += 3;
        } else {
            output.push(bytes[position]);
            position += 1;
        }
    }
    String::from_utf8(output).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_call_identities() {
        for call in ["call_1", "toolu_01A-b.c~d", "call/with space:é", "%41"] {
            let reference = Reference {
                call: call.into(),
                index: 12,
            };
            let text = reference.to_string();
            assert!(text.starts_with("sailry-image:"));
            assert!(!text.contains(' '));
            assert_eq!(Reference::parse(&text), Some(reference));
        }
        assert_eq!(
            Reference {
                call: "a/b".into(),
                index: 0
            }
            .to_string(),
            "sailry-image:a%2Fb/0"
        );
    }

    #[test]
    fn accepts_scheme_case() {
        assert_eq!(
            Reference::parse("Sailry-Image:call_1/2"),
            Some(Reference {
                call: "call_1".into(),
                index: 2
            })
        );
    }

    #[test]
    fn rejects_malformed_references() {
        for value in [
            "",
            "sailry-image:",
            "sailry-image:call_1",
            "sailry-image:/0",
            "sailry-image:call_1/",
            "sailry-image:call_1/01",
            "sailry-image:call_1/-1",
            "sailry-image:call_1/1.5",
            "sailry-image:call%2/0",
            "sailry-image:call%zz/0",
            "sailry-image:call%FF/0",
            "sailry-images:call_1/0",
            "https://example.com/call_1/0",
            "images/call_1/0",
        ] {
            assert_eq!(Reference::parse(value), None, "{value}");
        }
        let long = format!("sailry-image:{}/0", "a".repeat(MAX_CALL_BYTES + 1));
        assert_eq!(Reference::parse(&long), None);
    }
}
