use super::*;
use crate::process::output::{Buffer, Encoding, LIMIT, encoding};

fn buffer(number: u32, tail: bool) -> Buffer {
    Buffer {
        tail,
        decoder: encoding::Decoder::new(
            Encoding::code_page(number),
            if tail { 0 } else { LIMIT },
            if tail { LIMIT } else { 0 },
        ),
        ..Buffer::default()
    }
}

#[test]
fn decodes_host_code_pages_across_chunks() {
    for (number, raw, text) in [
        (
            936,
            b"\r\nMicrosoft Windows [\xb0\xe6\xb1\xbe 10.0.26100.9457]\r\n".as_slice(),
            "\r\nMicrosoft Windows [版本 10.0.26100.9457]\r\n",
        ),
        (932, b"\x82\xa0\x82\xa2".as_slice(), "あい"),
        (437, b"caf\x82".as_slice(), "café"),
    ] {
        assert!(CodePage::new(number).is_some());
        for tail in [false, true] {
            for size in 1..=raw.len() {
                let mut output = buffer(number, tail);
                for chunk in raw.chunks(size) {
                    output.append(chunk);
                    let capture = output.capture();
                    assert!(!capture.invalid_utf8, "{number}: {size}");
                    assert!(text.starts_with(&capture.text), "{number}: {size}");
                }
                output.finish();
                let capture = output.capture();
                assert_eq!(capture.text, text, "{number}: {size}");
                assert!(!capture.invalid_utf8);
            }
        }
    }
}

#[test]
fn finishes_single_byte_candidates() {
    for (number, raw, text) in [
        (1252, b"caf\xe9".as_slice(), "café"),
        (1252, b"\xc3".as_slice(), "Ã"),
        (437, b"value \xe9".as_slice(), "value Θ"),
        (437, b"\xef".as_slice(), "∩"),
    ] {
        for tail in [false, true] {
            for size in 1..=raw.len() {
                let mut output = buffer(number, tail);
                for chunk in raw.chunks(size) {
                    output.append(chunk);
                    let capture = output.capture();
                    assert!(!capture.invalid_utf8);
                    assert!(text.starts_with(&capture.text));
                }
                assert!(!output.decoder.transcoded());
                output.finish();
                output.finish();
                let capture = output.capture();
                assert_eq!(capture.text, text, "{number}: {size}");
                assert!(!capture.invalid_utf8);
                assert!(output.decoder.transcoded());
            }
        }
    }
}

#[tokio::test]
async fn finalizes_both_pipes() {
    use crate::process::output::Drain;
    use std::time::Duration;
    use tokio::io::AsyncWriteExt;

    let (mut stdout, stdout_reader) = tokio::io::duplex(64);
    let (mut stderr, stderr_reader) = tokio::io::duplex(64);
    let mut drain = Drain::with_encoding(
        stdout_reader,
        stderr_reader,
        false,
        None,
        Encoding::code_page(1252),
    );
    stdout.write_all(b"caf\xe9").await.unwrap();
    stderr.write_all(b"\xc3").await.unwrap();
    stdout.shutdown().await.unwrap();
    stderr.shutdown().await.unwrap();
    let (stdout, stderr, error) = drain.finish(Duration::from_secs(1)).await;
    assert!(error.is_none());
    assert_eq!(stdout.text, "café");
    assert_eq!(stderr.text, "Ã");
    assert!(!stdout.invalid_utf8 && !stderr.invalid_utf8);
}

#[test]
fn preserves_utf8_without_legacy_guessing() {
    let text = "Unicode 中文 🙂 あい café";
    for number in [936, 932, 437, 65001] {
        for size in 1..=text.len() {
            let mut output = buffer(number, false);
            for chunk in text.as_bytes().chunks(size) {
                output.append(chunk);
                let capture = output.capture();
                assert!(!capture.invalid_utf8);
                assert!(text.starts_with(&capture.text));
            }
            output.finish();
            assert_eq!(output.capture().text, text);
            assert!(!output.decoder.transcoded());
        }
    }
}

#[test]
fn honors_boms_across_chunks() {
    let text = "Unicode 中文 🙂 あい café";
    let mut utf8 = vec![0xef, 0xbb, 0xbf];
    utf8.extend_from_slice(text.as_bytes());
    let mut little = vec![0xff, 0xfe];
    let mut big = vec![0xfe, 0xff];
    for unit in text.encode_utf16() {
        little.extend_from_slice(&unit.to_le_bytes());
        big.extend_from_slice(&unit.to_be_bytes());
    }
    for raw in [utf8, little, big] {
        for size in 1..=raw.len() {
            let mut output = buffer(936, false);
            for chunk in raw.chunks(size) {
                output.append(chunk);
                let capture = output.capture();
                assert!(!capture.invalid_utf8, "{size}: {:?}", capture.text);
                assert!(text.starts_with(&capture.text));
            }
            output.finish();
            assert_eq!(output.capture().text, text);
            assert!(!output.capture().invalid_utf8);
            assert!(output.decoder.transcoded());
        }
    }
}

#[test]
fn reports_corrupt_and_incomplete_output() {
    for (number, raw) in [
        (936, b"\x81\x30".as_slice()),
        (936, b"\xb0".as_slice()),
        (936, b"\xe4\xb8".as_slice()),
        (437, b"caf\xc3\xa9\x82".as_slice()),
        (1252, b"caf\xc3\xa9\xe9".as_slice()),
        (1252, b"\xef\xbb\xbfcaf\xe9".as_slice()),
        (65001, b"caf\xe9".as_slice()),
        (936, b"\xef\xbb\xbf\xff".as_slice()),
        (936, b"\xff\xfe\x00\xd8".as_slice()),
        (936, b"\xfe\xff\x00".as_slice()),
        (65001, b"\xb0\xe6\xb1\xbe".as_slice()),
    ] {
        let mut output = buffer(number, false);
        for byte in raw {
            output.append(&[*byte]);
        }
        output.finish();
        let capture = output.capture();
        assert!(capture.invalid_utf8, "{number}: {raw:x?}");
        assert!(capture.text.contains('\u{fffd}'));
    }
}

#[test]
fn retains_bounded_source_boundaries() {
    let prefix = 17;
    let tail = 23;
    let raw = [b"x".as_slice(), b"\xb0\xe6\xb1\xbe".repeat(1000).as_slice()].concat();
    let mut decoder = Decoder::new(CodePage::new(936), prefix, tail);
    for chunk in raw.chunks(7) {
        decoder.append(chunk);
    }
    for offset in (0..=prefix).chain(raw.len() - tail..raw.len()) {
        assert_eq!(
            decoder.split(offset),
            offset > 0 && offset % 2 == 0,
            "{offset}"
        );
    }
    assert!(decoder.trails.len() <= (prefix + tail + 1).div_ceil(8));
}

#[test]
fn trims_head_and_tail_at_source_boundaries() {
    for tail in [false, true] {
        let mut output = buffer(936, tail);
        output.append(b"x");
        let repeated = b"\xb0\xe6\xb1\xbe".repeat(LIMIT);
        for bytes in repeated.chunks(8191) {
            output.append(bytes);
        }
        output.finish();
        let capture = output.capture();
        assert!(capture.truncated && !capture.invalid_utf8);
        if tail {
            assert_eq!(capture.text, "版本".repeat(LIMIT / 4));
        } else {
            assert_eq!(
                capture.text,
                format!("x{}版", "版本".repeat((LIMIT - 3) / 4))
            );
        }
    }
}
