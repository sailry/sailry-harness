use super::*;
use sailry_protocol::ErrorCode;

#[test]
fn respects_headers_meta_and_bom() {
    let original = "<head><meta charset='gbk'></head><body>中文</body>";
    let (gbk, _, failed) = encoding_rs::GBK.encode(original);
    assert!(!failed);
    assert_eq!(decode(&gbk, "text/html", true).unwrap(), original);
    let original = "<head><meta http-equiv='Content-Type' content='text/html; charset=GB18030'></head><body>中文𠀀</body>";
    let (bytes, _, failed) = encoding_rs::GB18030.encode(original);
    assert!(!failed);
    assert_eq!(decode(&bytes, "text/html", true).unwrap(), original);
    assert_eq!(
        decode(b"\xc2\xa3", "text/html; CHARSET = \"utf-8\"", true).unwrap(),
        "£"
    );
    assert!(decode(&gbk, "text/html; charset=utf-8", true).is_err());
    assert_eq!(
        decode(b"\xef\xbb\xbfUTF-8", "text/plain; charset=gbk", false).unwrap(),
        "UTF-8"
    );
    assert_eq!(
        decode(b"\xff\xfe\x2d\x4e\x87\x65", "text/plain", false).unwrap(),
        "中文"
    );
}

#[test]
fn preserves_json_and_fallback_text() {
    let original = "{\r\n  \"message\": \"中文\", \"count\": 1.00\r\n}\n";
    let (bytes, _, failed) = encoding_rs::GBK.encode(original);
    assert!(!failed);
    assert_eq!(
        decode(&bytes, "application/json; charset=gbk", false).unwrap(),
        original
    );
    assert_eq!(decode(&bytes, "text/plain", false).unwrap(), original);
    let original = "<meta charset='windows-1252'><p>£</p>";
    let (bytes, _, failed) = encoding_rs::WINDOWS_1252.encode(original);
    assert!(!failed);
    assert_eq!(decode(&bytes, "text/html", true).unwrap(), original);
}

#[test]
fn rejects_invalid_and_binary_content() {
    for (bytes, content_type) in [
        (b"\xff".as_slice(), "text/plain; charset=utf-8"),
        (b"\x81".as_slice(), "text/plain; charset=gbk"),
        (b"text".as_slice(), "text/plain; charset=unknown"),
        (b"a\0b".as_slice(), "text/plain; charset=utf-8"),
    ] {
        assert_eq!(
            decode(bytes, content_type, false).unwrap_err().code,
            ErrorCode::Unavailable
        );
    }
    let late = format!("{}<meta charset=windows-1252>£", " ".repeat(1024));
    let (bytes, _, _) = encoding_rs::WINDOWS_1252.encode(&late);
    assert!(decode(&bytes, "text/html", true).is_err());
}
