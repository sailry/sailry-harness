use super::*;
#[test]
fn accepts_partial_and_adjacent_frames() {
    let mut codec = Codec::default();
    let mut bytes = BytesMut::new();
    codec
        .encode(serde_json::json!({"id":1}), &mut bytes)
        .unwrap();
    codec
        .encode(serde_json::json!({"id":2}), &mut bytes)
        .unwrap();
    let mut partial = bytes.split_to(10);
    assert!(codec.decode(&mut partial).unwrap().is_none());
    partial.extend_from_slice(&bytes);
    assert_eq!(codec.decode(&mut partial).unwrap().unwrap()["id"], 1);
    assert_eq!(codec.decode(&mut partial).unwrap().unwrap()["id"], 2);
    assert!(partial.is_empty());
}
#[test]
fn rejects_duplicate_missing_and_oversized_lengths() {
    for headers in [
        "Content-Length: 2\r\nContent-Length: 2\r\n\r\n{}",
        "Content-Type: application/json\r\n\r\n{}",
        "Content-Length: 8388609\r\n\r\n",
        "Content-Length: 0\r\n\r\n",
        "Content-Length: -1\r\n\r\n",
    ] {
        assert!(
            Codec::default()
                .decode(&mut BytesMut::from(headers))
                .is_err()
        );
    }
    assert!(
        Codec::default()
            .decode(&mut BytesMut::from(vec![b'x'; HEADERS + 1].as_slice()))
            .is_err()
    );
}
