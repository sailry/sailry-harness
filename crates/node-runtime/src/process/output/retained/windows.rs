use super::*;
use crate::process::output::Encoding;

fn capture(raw: &[u8], number: u32) -> (tempfile::TempDir, Retained) {
    let profile = tempfile::tempdir().unwrap();
    let retained = Retained::new(profile.path(), Arc::default());
    retained.configure(Encoding::code_page(number));
    for bytes in raw.chunks(1) {
        retained.append(OutputStream::Stdout, bytes);
    }
    retained.finish();
    (profile, retained)
}

#[test]
fn pages_encoded_output_at_raw_offsets() {
    for (number, raw, text, boundaries) in [
        (936, b"\xb0\xe6\xb1\xbe".as_slice(), "版本", vec![0, 2, 4]),
        (932, b"\x82\xa0\x82\xa2".as_slice(), "あい", vec![0, 2, 4]),
        (437, b"caf\x82".as_slice(), "café", vec![0, 1, 2, 3, 4]),
        (1252, b"caf\xe9".as_slice(), "café", vec![0, 1, 2, 3, 4]),
        (437, b"\xe9".as_slice(), "Θ", vec![0, 1]),
        (
            936,
            b"\xff\xfe\x48\x72\x3d\xd8\x42\xde".as_slice(),
            "版🙂",
            vec![0, 2, 4, 8],
        ),
        (
            936,
            b"\xfe\xff\x72\x48\xd8\x3d\xde\x42".as_slice(),
            "版🙂",
            vec![0, 2, 4, 8],
        ),
        (
            936,
            b"\xef\xbb\xbf\xe7\x89\x88".as_slice(),
            "版",
            vec![0, 3, 6],
        ),
    ] {
        let (_profile, retained) = capture(raw, number);
        let stored = retained.stdout.lock().unwrap();
        assert_eq!(std::fs::read(stored.path.as_ref().unwrap()).unwrap(), raw);
        drop(stored);
        for limit in [1, 3, 5, PAGE] {
            let mut offset = 0;
            let mut actual = String::new();
            loop {
                let page = retained
                    .page(tests::info(), OutputStream::Stdout, offset, limit)
                    .unwrap();
                assert_eq!(page.offset, offset);
                assert_eq!(page.total_bytes, raw.len() as u64);
                assert!(!page.invalid_utf8 && !page.truncated);
                actual.push_str(&page.text);
                let Some(next) = page.next_offset else { break };
                assert!(
                    next > offset && boundaries.contains(&next),
                    "{number}: {next}"
                );
                offset = next;
            }
            assert_eq!(actual, text, "{number}: {limit}");
        }
        for offset in 0..raw.len() {
            if !boundaries.contains(&offset) {
                assert!(
                    retained
                        .page(tests::info(), OutputStream::Stdout, offset, PAGE)
                        .is_err()
                );
            }
        }
        let preview = retained.preview(tests::info().id).unwrap();
        assert_eq!(preview["stdout"]["transcoded"], true);
        assert_eq!(preview["stdout"]["total_bytes"], raw.len());
    }
}

#[test]
fn waits_for_live_multibyte_suffixes() {
    for (raw, text) in [
        (b"\xb0\xe6\xb1\xbe".as_slice(), "版本"),
        ("中文🙂".as_bytes(), "中文🙂"),
        (b"\xff\xfe\x48\x72\x3d\xd8\x42\xde".as_slice(), "版🙂"),
    ] {
        let profile = tempfile::tempdir().unwrap();
        let retained = Retained::new(profile.path(), Arc::default());
        retained.configure(Encoding::code_page(936));
        for byte in raw {
            retained.append(OutputStream::Stdout, &[*byte]);
            let page = retained
                .page(tests::info(), OutputStream::Stdout, 0, PAGE)
                .unwrap();
            assert!(!page.invalid_utf8);
            assert!(text.starts_with(&page.text));
            let recent = retained.recent().0;
            assert!(!recent.invalid_utf8);
            assert_eq!(recent.text, page.text);
        }
        retained.finish();
        assert_eq!(
            retained
                .page(tests::info(), OutputStream::Stdout, 0, PAGE)
                .unwrap()
                .text,
            text
        );
    }
}

#[test]
fn separates_stream_encoding_and_failures() {
    let profile = tempfile::tempdir().unwrap();
    let retained = Retained::new(profile.path(), Arc::default());
    retained.configure(Encoding::code_page(936));
    retained.append(OutputStream::Stdout, b"\xb0\xe6\xb1\xbe");
    retained.append(OutputStream::Stderr, "中文🙂".as_bytes());
    retained.finish();
    for (stream, text, total) in [
        (OutputStream::Stdout, "版本", 4),
        (OutputStream::Stderr, "中文🙂", 10),
    ] {
        let page = retained.page(tests::info(), stream, 0, PAGE).unwrap();
        assert_eq!(page.text, text);
        assert_eq!(page.total_bytes, total);
        assert!(!page.invalid_utf8);
    }
    let mixed = [b"\xb0\xe6".as_slice(), "中".as_bytes()].concat();
    let (_profile, retained) = capture(&mixed, 936);
    assert!(
        retained
            .page(tests::info(), OutputStream::Stdout, 2, PAGE)
            .unwrap()
            .invalid_utf8
    );
    let (_profile, retained) = capture(b"\xb0\xe6\x81\x30", 936);
    assert!(
        retained
            .page(tests::info(), OutputStream::Stdout, 0, PAGE)
            .unwrap()
            .invalid_utf8
    );
}

#[test]
fn keeps_contiguous_and_rolling_boundaries() {
    for length in [PREFIX_LIMIT + 9, STREAM_LIMIT + 9] {
        let mut raw = b"x".to_vec();
        while raw.len() < length {
            raw.extend_from_slice(b"\xb0\xe6\xb1\xbe");
        }
        let profile = tempfile::tempdir().unwrap();
        let retained = Retained::new(profile.path(), Arc::default());
        retained.configure(Encoding::code_page(936));
        for chunk in raw.chunks(8191) {
            retained.append(OutputStream::Stdout, chunk);
        }
        retained.finish();
        let mut actual = String::new();
        let mut offset = 0;
        let mut jumped = false;
        loop {
            let page = retained
                .page(tests::info(), OutputStream::Stdout, offset, PAGE - 1)
                .unwrap();
            assert!(!page.invalid_utf8);
            assert_eq!(page.offset, offset);
            assert_eq!(page.truncated, raw.len() > STREAM_LIMIT);
            assert_eq!(page.total_bytes, raw.len() as u64);
            let end = offset
                + page
                    .text
                    .chars()
                    .map(|character| if character.is_ascii() { 1 } else { 2 })
                    .sum::<usize>();
            actual.push_str(&page.text);
            let Some(next) = page.next_offset else {
                assert_eq!(end, raw.len());
                break;
            };
            assert!(next > offset);
            if next > end {
                assert_eq!(end, PREFIX_LIMIT - 1);
                assert_eq!(next, raw.len() - (STREAM_LIMIT - PREFIX_LIMIT));
                jumped = true;
            } else {
                assert_eq!(next, end);
            }
            offset = next;
        }
        let preview = retained.preview(tests::info().id).unwrap();
        let preview = &preview["stdout"];
        let tail_offset = preview["tail_offset"].as_u64().unwrap() as usize;
        assert_eq!(tail_offset % 2, 1);
        assert_eq!(preview["transcoded"], true);
        assert_eq!(preview["tail"], "版本".repeat(TAIL / 4));
        if raw.len() <= STREAM_LIMIT {
            assert!(!jumped);
            assert_eq!(actual, format!("x{}", "版本".repeat((raw.len() - 1) / 4)));
        } else {
            assert!(jumped);
            let head = preview["retained_head_bytes"].as_u64().unwrap() as usize;
            let tail = preview["retained_tail_offset"].as_u64().unwrap() as usize;
            assert_eq!(head, PREFIX_LIMIT - 1);
            assert_eq!(tail, raw.len() - (STREAM_LIMIT - PREFIX_LIMIT));
            assert_eq!(
                actual,
                format!(
                    "x{}版{}",
                    "版本".repeat((head - 3) / 4),
                    "版本".repeat((raw.len() - tail) / 4)
                )
            );
            let page = retained
                .page(tests::info(), OutputStream::Stdout, head, 1)
                .unwrap();
            assert_eq!(page.offset, tail);
            assert_eq!(page.next_offset, Some(tail + 2));
        }
    }
}

#[test]
fn freezes_finished_decoder_and_boundaries() {
    let raw = b"\xb0\xe6\xb1\xbe";
    let (_profile, retained) = capture(raw, 936);
    retained.append(OutputStream::Stdout, &b"\x81\x30".repeat(STREAM_LIMIT));
    let page = retained
        .page(tests::info(), OutputStream::Stdout, 0, 2)
        .unwrap();
    assert_eq!(page.text, "版");
    assert!(!page.invalid_utf8);
    assert_eq!(page.next_offset, Some(2));
    assert!(page.truncated);
    let (_profile, retained) = capture(b"\xb0", 936);
    retained.append(OutputStream::Stdout, b"later");
    let page = retained
        .page(tests::info(), OutputStream::Stdout, 0, PAGE)
        .unwrap();
    assert!(page.invalid_utf8);
    assert_eq!(page.text, "�");
}
