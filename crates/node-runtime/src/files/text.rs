//! Strict text decoding shared by confined search and knowledge extraction.
use std::borrow::Cow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Binary,
    Unsupported,
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Binary => "binary content cannot be decoded as text",
            Self::Unsupported => "text is neither valid UTF-8 nor GB18030",
        })
    }
}

pub(crate) fn decode(bytes: &[u8]) -> Result<Cow<'_, str>, Error> {
    if bytes
        .iter()
        .any(|byte| *byte < 0x20 && !matches!(*byte, b'\t' | b'\n' | b'\r' | 0x0c) || *byte == 0x7f)
    {
        return Err(Error::Binary);
    }
    let decoded = if let Some(bytes) = bytes.strip_prefix(b"\xef\xbb\xbf") {
        Cow::Borrowed(std::str::from_utf8(bytes).map_err(|_| Error::Unsupported)?)
    } else if let Ok(text) = std::str::from_utf8(bytes) {
        Cow::Borrowed(text)
    } else {
        encoding_rs::GB18030
            .decode_without_bom_handling_and_without_replacement(bytes)
            .ok_or(Error::Unsupported)?
    };
    if decoded.chars().any(|character| {
        character.is_control() && !matches!(character, '\t' | '\n' | '\r' | '\u{c}')
    }) {
        return Err(Error::Binary);
    }
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_utf8_and_strips_its_bom() {
        let original = "中文\r\ntext\t🙂";
        assert_eq!(decode(original.as_bytes()).unwrap(), original);
        assert!(matches!(decode(b"plain text").unwrap(), Cow::Borrowed(_)));
        let prefixed = format!("\u{feff}{original}");
        assert_eq!(decode(prefixed.as_bytes()).unwrap(), original);
        assert_eq!(decode(b"\xef\xbb\xbf\xd6\xd0"), Err(Error::Unsupported));
    }

    #[test]
    fn decodes_gbk_and_four_byte_gb18030() {
        assert_eq!(decode(b"\xd6\xd0\xce\xc4\r\ntext").unwrap(), "中文\r\ntext");
        let original = "中文𠀀🙂";
        let (bytes, _, failed) = encoding_rs::GB18030.encode(original);
        assert!(!failed);
        assert_ne!(&*bytes, original.as_bytes());
        assert_eq!(decode(&bytes).unwrap(), original);
    }

    #[test]
    fn rejects_binary_and_malformed_sequences() {
        for bytes in [b"text\0binary".as_slice(), b"\x01text", b"\x7ftext"] {
            assert_eq!(decode(bytes), Err(Error::Binary));
        }
        assert_eq!(decode("text\u{85}".as_bytes()), Err(Error::Binary));
        for bytes in [b"text\xff".as_slice(), b"\x81", b"\x81\x30\x81"] {
            assert_eq!(decode(bytes), Err(Error::Unsupported));
        }
    }
}
