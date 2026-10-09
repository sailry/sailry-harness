use super::*;

fn varint(mut value: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    while value >= 128 {
        bytes.push((value as u8 & 127) | 128);
        value >>= 7;
    }
    bytes.push(value as u8);
    bytes
}

fn field(tag: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = varint(u64::from(tag) << 3 | 2);
    bytes.extend(varint(payload.len() as u64));
    bytes.extend_from_slice(payload);
    bytes
}

fn root(payload: &[u8]) -> Vec<u8> {
    let mut bytes = field(7, &[]);
    bytes.extend_from_slice(payload);
    bytes
}

fn attributes(attribute: &[u8]) -> Vec<u8> {
    field(1, &field(5, attribute))
}

fn paths(tensor: &[u8]) -> Vec<Vec<u8>> {
    let graph = field(5, tensor);
    let mut paths = vec![
        field(7, &graph),
        field(20, &field(1, &graph)),
        field(20, &field(2, &graph)),
    ];
    for tag in [5, 10] {
        let attribute = field(tag, tensor);
        paths.push(field(7, &attributes(&attribute)));
        paths.push(field(25, &field(7, &field(5, &attribute))));
        paths.push(field(25, &field(11, &attribute)));
    }
    for tag in [6, 11] {
        paths.push(field(7, &attributes(&field(tag, &graph))));
    }
    for tag in [1, 2] {
        let sparse = field(tag, tensor);
        paths.push(field(7, &field(15, &sparse)));
        for tag in [22, 23] {
            paths.push(field(7, &attributes(&field(tag, &sparse))));
        }
    }
    paths
}

#[test]
fn rejects_external_references_in_every_container() {
    for tensor in [
        field(13, &[]),
        vec![0x70, 1],
        vec![0x70, 1, 0x70, 0],
        vec![0x70, 0, 0x70, 1],
    ] {
        for payload in paths(&tensor) {
            let error = check(&root(&payload)).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert!(error.to_string().contains("external tensor data"));
        }
    }
}

#[test]
fn does_not_interpret_weight_bytes_as_metadata() {
    let raw = field(9, &[0x6a, 0, 0x70, 1, 0x70, 0]);
    for payload in paths(&raw) {
        check(&root(&payload)).unwrap();
    }
}

#[test]
fn validates_owned_file_without_external_reads() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("model.onnx");
    std::fs::write(&path, root(&field(7, &field(5, &field(9, b"weights"))))).unwrap();
    validate(&path).unwrap();
    let external = [field(1, b"location"), field(2, b"../outside")].concat();
    std::fs::write(&path, root(&field(7, &field(5, &field(13, &external))))).unwrap();
    let error = validate(&path).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("external tensor data"));
}

#[test]
fn rejects_malformed_and_deep_metadata() {
    for bytes in [
        vec![],
        vec![0],
        vec![0x3a, 0x80],
        root(&field(7, &[0x2a, 0xff, 0xff])),
    ] {
        assert_eq!(
            check(&bytes).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
    let mut graph = Vec::new();
    for _ in 0..64 {
        graph = attributes(&field(6, &graph));
    }
    assert_eq!(
        check(&field(7, &graph)).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
}
