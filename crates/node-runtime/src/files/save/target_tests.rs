use super::*;
use sailry_protocol::FILE_TRANSFER_CHUNK_BYTES;

struct Bounded<'a>(&'a [u8]);
impl Read for Bounded<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        assert!(bytes.len() <= FILE_TRANSFER_CHUNK_BYTES);
        self.0.read(bytes)
    }
}

#[test]
fn streams_binary_replacements() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let bytes = vec![0xf1; 10 * 1024 * 1024];
    let revision = blake3::hash(&bytes).to_hex().to_string();
    let output = Target::prepare(&root, "资料.bin", None, u64::MAX, None)
        .unwrap()
        .write(Bounded(&bytes), bytes.len() as u64, &revision, &|_| {})
        .unwrap();
    assert_eq!(output.size, bytes.len() as u64);
    assert_eq!(std::fs::read(root.join("资料.bin")).unwrap(), bytes);
    // The editor's original bounded-read contract remains intact.
    assert_eq!(
        save(&root, "资料.bin", "text", Some(&revision))
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    let empty = blake3::hash(&[]).to_hex().to_string();
    let output = Target::prepare(&root, "资料.bin", Some(&revision), u64::MAX, None)
        .unwrap()
        .write(Bounded(&[]), 0, &empty, &|_| {})
        .unwrap();
    assert_eq!(output.size, 0);
    assert!(std::fs::read(root.join("资料.bin")).unwrap().is_empty());
    assert_eq!(std::fs::read_dir(root).unwrap().count(), 1);
}

#[test]
fn rejects_mismatched_streams() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let path = root.join("file");
    let original = blake3::hash(b"original").to_hex().to_string();
    std::fs::write(&path, b"original").unwrap();
    let revision = blake3::hash(b"valid").to_hex().to_string();
    for (bytes, size) in [(b"valid".as_slice(), 2), (b"valid", 6), (b"other", 5)] {
        let target = Target::prepare(&root, "file", Some(&original), u64::MAX, None).unwrap();
        assert_eq!(
            target
                .write(Bounded(bytes), size, &revision, &|_| {})
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
    }
}

#[test]
fn reports_uncertain_creation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let revision = blake3::hash(b"larger").to_hex().to_string();
    let error = Target::prepare(&root, "file", None, u64::MAX, None)
        .unwrap()
        .write(Bounded(b"part"), 6, &revision, &|_| {})
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::OutcomeUnknown);
    assert_eq!(std::fs::read(root.join("file")).unwrap(), b"part");
}

#[test]
fn cancels_before_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::write(root.join("file"), b"original").unwrap();
    let revision = blake3::hash(b"original").to_hex().to_string();
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(matches!(
        Target::prepare(&root, "file", Some(&revision), u64::MAX, Some(&cancel)),
        Err(Fault {
            code: ErrorCode::Cancelled,
            ..
        })
    ));
    assert_eq!(std::fs::read(root.join("file")).unwrap(), b"original");
    assert_eq!(std::fs::read_dir(root).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn rejects_recreated_parent() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::create_dir(root.join("parent")).unwrap();
    let target = Target::prepare(&root, "parent/file", None, u64::MAX, None).unwrap();
    std::fs::rename(root.join("parent"), root.join("moved")).unwrap();
    std::fs::create_dir(root.join("parent")).unwrap();
    assert_eq!(
        target
            .write(
                Bounded(b"bytes"),
                5,
                &blake3::hash(b"bytes").to_hex(),
                &|_| {}
            )
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    assert_eq!(std::fs::read_dir(root.join("parent")).unwrap().count(), 0);
    assert_eq!(std::fs::read_dir(root.join("moved")).unwrap().count(), 0);
}
