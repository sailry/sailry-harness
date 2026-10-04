use super::*;
use crate::profile::Profile;
use windows_sys::Win32::Security::{
    Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
    GetSecurityDescriptorLength, PROTECTED_DACL_SECURITY_INFORMATION, SetFileSecurityW,
};

#[test]
fn creates_private_profile_and_inherited_files() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("用户 🙂");
    let profile = Profile::acquire(&path).unwrap();
    let key = profile.path.join("link.key");
    std::fs::write(&key, b"isolated identity fixture").unwrap();
    validate_existing_file(&key).unwrap();
    let database = profile.path.join("storage/node.sqlite3");
    prepare_database(&database).unwrap();
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut sidecar = database.as_os_str().to_owned();
        sidecar.push(suffix);
        std::fs::write(sidecar, b"isolated journal fixture").unwrap();
    }
    validate_database(&database).unwrap();
    drop(profile);
    Profile::acquire(&path).unwrap();
}

#[test]
fn rejects_shared_directory_without_modifying_it() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("shared");
    create(&path, "D:P(A;OICI;FA;;;WD)");
    let marker = path.join("existing.bin");
    std::fs::write(&marker, b"preserved data").unwrap();
    let before = descriptor(&path);
    assert!(Profile::acquire(&path).is_err());
    assert_eq!(descriptor(&path), before);
    assert_eq!(std::fs::read(&marker).unwrap(), b"preserved data");
    assert!(!path.join("node.lock").exists());
    assert!(!path.join("storage").exists());
}

#[test]
fn rejects_shared_identity_before_lock_creation() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("profile");
    directory(&path).unwrap();
    let key = path.join("link.key");
    std::fs::write(&key, b"preserved identity").unwrap();
    share(&key);
    let before = descriptor(&key);
    assert!(Profile::acquire(&path).is_err());
    assert_eq!(descriptor(&key), before);
    assert_eq!(std::fs::read(&key).unwrap(), b"preserved identity");
    assert!(!path.join("node.lock").exists());
}

#[test]
fn rejects_shared_lock_without_modifying_it() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("profile");
    directory(&path).unwrap();
    let lock = path.join("node.lock");
    std::fs::write(&lock, b"preserved lock").unwrap();
    share(&lock);
    let before = descriptor(&lock);
    assert!(Profile::acquire(&path).is_err());
    assert_eq!(descriptor(&lock), before);
    assert_eq!(std::fs::read(&lock).unwrap(), b"preserved lock");
}

#[test]
fn rejects_shared_database_without_modifying_it() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("storage");
    directory(&path).unwrap();
    let database = path.join("node.sqlite3");
    prepare_database(&database).unwrap();
    std::fs::write(&database, b"preserved database").unwrap();
    share(&database);
    let before = descriptor(&database);
    assert!(prepare_database(&database).is_err());
    assert_eq!(descriptor(&database), before);
    assert_eq!(std::fs::read(&database).unwrap(), b"preserved database");
}

#[test]
fn rejects_shared_journal_before_creating_database() {
    for suffix in ["-wal", "-shm", "-journal"] {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("storage");
        directory(&path).unwrap();
        let database = path.join("node.sqlite3");
        let sidecar = path.join(format!("node.sqlite3{suffix}"));
        std::fs::write(&sidecar, b"preserved journal").unwrap();
        share(&sidecar);
        let before = descriptor(&sidecar);
        assert!(prepare_database(&database).is_err());
        assert_eq!(descriptor(&sidecar), before);
        assert_eq!(std::fs::read(&sidecar).unwrap(), b"preserved journal");
        assert!(!database.exists());
    }
}

#[test]
fn rejects_hard_linked_database() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("storage");
    directory(&path).unwrap();
    let database = path.join("node.sqlite3");
    prepare_database(&database).unwrap();
    std::fs::write(&database, b"preserved database").unwrap();
    let link = path.join("alias.sqlite3");
    std::fs::hard_link(&database, &link).unwrap();
    assert!(prepare_database(&database).is_err());
    assert_eq!(std::fs::read(&link).unwrap(), b"preserved database");
}

#[test]
fn rejects_null_access_list() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("unrestricted");
    create(&path, "D:NO_ACCESS_CONTROL");
    assert!(Profile::acquire(&path).is_err());
}

fn create(path: &Path, sddl: &str) {
    let security = sddl_descriptor(sddl);
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: security.0,
        bInheritHandle: 0,
    };
    let name = wide(path).unwrap();
    // SAFETY: both terminated name and descriptor remain live for the call.
    check(unsafe { CreateDirectoryW(name.as_ptr(), &attributes) }).unwrap();
}

fn share(path: &Path) {
    let security = sddl_descriptor("D:P(A;OICI;FA;;;WD)");
    let name = wide(path).unwrap();
    // SAFETY: these pointers reference a live name and validated descriptor.
    check(unsafe {
        SetFileSecurityW(
            name.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            security.0,
        )
    })
    .unwrap();
}

fn sddl_descriptor(sddl: &str) -> Allocation {
    let text: Vec<_> = sddl.encode_utf16().chain(Some(0)).collect();
    let mut descriptor = null_mut();
    // SAFETY: text is terminated; the API returns an owning LocalAlloc block.
    check(unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text.as_ptr(),
            SECURITY_DESCRIPTOR_REVISION,
            &mut descriptor,
            null_mut(),
        )
    })
    .unwrap();
    Allocation(descriptor)
}

fn descriptor(path: &Path) -> Vec<u8> {
    let file = OpenOptions::new()
        .access_mode(READ_CONTROL)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .unwrap();
    let security = Security::read(&file).unwrap();
    // SAFETY: the self-relative descriptor and its entire allocation are live.
    unsafe {
        let length = GetSecurityDescriptorLength(security.allocation.0) as usize;
        std::slice::from_raw_parts(security.allocation.0.cast::<u8>(), length).to_vec()
    }
}
