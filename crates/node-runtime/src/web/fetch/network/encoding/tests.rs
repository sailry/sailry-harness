use super::*;

#[test]
fn respects_headers_meta_and_bom() {
    let original = "<head><meta charset='gbk'></head><body>中文</body>";
    let (gbk, _, failed) = encoding_rs::GBK.encode(original);
    assert!(!failed);
    assert_eq!(decode(&gbk, "text/html", true), original);
    let original = "<head><meta http-equiv='Content-Type' content='text/html; charset=GB18030'></head><body>中文𠀀</body>";
    let (bytes, _, failed) = encoding_rs::GB18030.encode(original);
    assert!(!failed);
    assert_eq!(decode(&bytes, "text/html", true), original);
    assert_eq!(
        decode(b"\xc2\xa3", "text/html; CHARSET = \"utf-8\"", true),
        "£"
    );
    assert!(decode(&gbk, "text/html; charset=utf-8", true).contains("\u{fffd}"));
    assert_eq!(
        decode(b"\xef\xbb\xbfUTF-8", "text/plain; charset=gbk", false),
        "UTF-8"
    );
    assert_eq!(
        decode(b"\xff\xfe\x2d\x4e\x87\x65", "text/plain", false),
        "中文"
    );
}

#[test]
fn preserves_json_and_fallback_text() {
    let original = "{\r\n  \"message\": \"中文\", \"count\": 1.00\r\n}\n";
    let (bytes, _, failed) = encoding_rs::GBK.encode(original);
    assert!(!failed);
    assert_eq!(
        decode(&bytes, "application/json; charset=gbk", false),
        original
    );
    assert_eq!(decode(&bytes, "text/plain", false), original);
    let original = "<meta charset='windows-1252'><p>£</p>";
    let (bytes, _, failed) = encoding_rs::WINDOWS_1252.encode(original);
    assert!(!failed);
    assert_eq!(decode(&bytes, "text/html", true), original);
}

#[test]
fn replaces_invalid_bytes_and_controls() {
    for (bytes, content_type, expected) in [
        (b"\xff".as_slice(), "text/plain; charset=utf-8", "�"),
        (b"\x81".as_slice(), "text/plain; charset=gbk", "�"),
        (b"text".as_slice(), "text/plain; charset=unknown", "text"),
        (b"a\0b".as_slice(), "text/plain; charset=utf-8", "a�b"),
        (b"a\xffb".as_slice(), "text/plain", "a�b"),
    ] {
        assert_eq!(decode(bytes, content_type, false), expected);
    }
    let late = format!("{}<meta charset=windows-1252>£", " ".repeat(1024));
    let (bytes, _, _) = encoding_rs::WINDOWS_1252.encode(&late);
    assert!(decode(&bytes, "text/html", true).ends_with("�"));
}
