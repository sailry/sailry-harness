use super::*;
use crate::backup::tests::snapshot;
use std::io::Write;

fn phrase() -> Secret {
    Secret::new("private backup fixture phrase".into())
}

#[test]
fn authenticates_before_unpacking() {
    let (_parent, snapshot) = snapshot();
    let cancel = CancellationToken::new();
    let plain = snapshot.pack(&cancel).unwrap();
    let previous = std::fs::read(plain.file.path()).unwrap();
    let packed = seal(plain, &phrase(), &cancel).unwrap();
    let encrypted = std::fs::read(packed.file.path()).unwrap();
    assert!(encrypted.starts_with(b"age-encryption.org/v1\n"));
    assert_ne!(encrypted, previous);
    assert_eq!(packed.size, encrypted.len() as u64);
    assert_eq!(
        archive::hash(&mut packed.file.reopen().unwrap(), packed.size, &cancel).unwrap(),
        packed.sha256
    );
    let decrypted = open(&packed.file, &phrase(), &cancel).unwrap();
    assert_eq!(std::fs::read(decrypted.path()).unwrap(), previous);
    let target = tempfile::tempdir().unwrap();
    let root = target.path().canonicalize().unwrap();
    archive::unpack(decrypted.reopen().unwrap(), &root, &cancel).unwrap();
    assert_eq!(
        std::fs::read(target.path().join("storage/node.sqlite3")).unwrap(),
        b"consistent database fixture"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            decrypted.as_file().metadata().unwrap().permissions().mode() & 0o077,
            0
        );
        assert_eq!(
            packed
                .file
                .as_file()
                .metadata()
                .unwrap()
                .permissions()
                .mode()
                & 0o077,
            0
        );
    }
}

#[test]
fn rejects_wrong_keys_and_damaged_streams() {
    let (_parent, snapshot) = snapshot();
    let cancel = CancellationToken::new();
    let packed = seal(snapshot.pack(&cancel).unwrap(), &phrase(), &cancel).unwrap();
    let entries = std::fs::read_dir(packed.file.path().parent().unwrap())
        .unwrap()
        .count();
    let wrong = Secret::new("wrong private fixture phrase".into());
    assert_eq!(
        open(&packed.file, &wrong, &cancel).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        std::fs::read_dir(packed.file.path().parent().unwrap())
            .unwrap()
            .count(),
        entries
    );
    let original = std::fs::read(packed.file.path()).unwrap();
    for truncated in [false, true] {
        let mut bytes = original.clone();
        if truncated {
            bytes.pop();
        } else {
            *bytes.last_mut().unwrap() ^= 1;
        }
        let broken = tempfile::NamedTempFile::new().unwrap();
        broken.as_file().write_all(&bytes).unwrap();
        let before = std::fs::read(broken.path()).unwrap();
        assert_eq!(
            open(&broken, &phrase(), &cancel).unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(std::fs::read(broken.path()).unwrap(), before);
    }
}

#[test]
fn cancels_without_creating_output() {
    let (_parent, snapshot) = snapshot();
    let packed = snapshot.pack(&CancellationToken::new()).unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        seal(packed, &phrase(), &cancel).err().unwrap().code,
        ErrorCode::Cancelled
    );
    assert_eq!(passphrase(None).unwrap_err().code, ErrorCode::NotConfigured);
    assert!(passphrase(Some(&Secret::new(String::new()))).is_err());
}
