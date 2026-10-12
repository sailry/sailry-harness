use super::*;

#[test]
fn encodes_bounded_payload_with_a_quiet_zone() {
    let bitmap = super::super::qr::render("Sailry card").unwrap();
    assert!(bitmap.width >= 116 && bitmap.width <= 768);
    let pixels = bitmap.image.as_bytes(0).unwrap();
    assert_eq!(pixels.len(), (bitmap.width * bitmap.width * 4) as usize);
    // Four quiet modules, at four pixels per module, remain fully opaque white.
    assert!(
        pixels[..(bitmap.width * 16 * 4) as usize]
            .iter()
            .all(|byte| *byte == 255)
    );
    assert!(super::super::qr::render("").is_err());
    assert!(
        super::super::qr::render(&"x".repeat(sailry_protocol::plugin::card::MAX_QR_BYTES + 1))
            .is_err()
    );
    assert!(!super::super::qr::live(Some(100), 100));
    assert!(super::super::qr::live(Some(101), 100));
}
