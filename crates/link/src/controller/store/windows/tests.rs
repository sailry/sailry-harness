use super::*;
use crate::{PeerStore, controller::store::Store};
use sailry_protocol::NodeId;
use windows_sys::Win32::Security::{
    Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
    GetSecurityDescriptorLength, PROTECTED_DACL_SECURITY_INFORMATION, SetFileSecurityW,
};

#[tokio::test]
async fn creates_private_storage_and_reloads() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("用户 🙂");
    let (identity, store) = Store::open(&path).unwrap();
    let peer = NodeId([1; 32]);
    store.set_trust(peer, true).await.unwrap();
    for name in ["node.lock", "link.key", "peers.json"] {
        validate_existing_file(&path.join(name)).unwrap();
    }
    store.close().await.unwrap();
    let (reloaded, reopened) = Store::open(&path).unwrap();
    assert_eq!(reloaded.id(), identity.id());
    assert_eq!(reopened.peers().await.unwrap(), vec![peer]);
    reopened.close().await.unwrap();
}

#[test]
fn preserves_shared_directory() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("shared");
    create(&path, "D:P(A;OICI;FA;;;WD)");
    let marker = path.join("marker");
    std::fs::write(&marker, b"preserved data").unwrap();
    let before = descriptor(&path);
    assert!(Store::open(&path).is_err());
    assert_eq!(descriptor(&path), before);
    assert_eq!(std::fs::read(marker).unwrap(), b"preserved data");
    assert!(!path.join("node.lock").exists());
    assert!(!path.join("link.key").exists());
}

#[test]
fn preserves_shared_files() {
    for name in ["node.lock", "link.key", "peers.json"] {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("profile");
        let (_, store) = Store::open(&path).unwrap();
        drop(store);
        let file = path.join(name);
        let bytes = std::fs::read(&file).unwrap();
        share(&file);
        let before = descriptor(&file);
        assert!(Store::open(&path).is_err(), "accepted {name}");
        assert_eq!(descriptor(&file), before);
        assert_eq!(std::fs::read(file).unwrap(), bytes);
    }
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
    assert!(Store::open(&path).is_err());
    assert_eq!(descriptor(&key), before);
    assert_eq!(std::fs::read(key).unwrap(), b"preserved identity");
    assert!(!path.join("node.lock").exists());
}

#[test]
fn rejects_hard_linked_files() {
    for name in ["node.lock", "link.key", "peers.json"] {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("profile");
        let (_, store) = Store::open(&path).unwrap();
        drop(store);
        let file = path.join(name);
        let before = std::fs::read(&file).unwrap();
        let alias = path.join("alias");
        std::fs::hard_link(&file, &alias).unwrap();
        assert!(Store::open(&path).is_err(), "accepted {name}");
        assert_eq!(std::fs::read(file).unwrap(), before);
        assert_eq!(std::fs::read(alias).unwrap(), before);
    }
}

#[test]
fn protects_live_lock_from_replacement() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("profile");
    let (_, store) = Store::open(&path).unwrap();
    let lock = path.join("node.lock");
    assert!(std::fs::remove_file(&lock).is_err());
    assert!(std::fs::rename(&lock, path.join("renamed.lock")).is_err());
    assert!(Store::open(&path).is_err());
    drop(store);
    Store::open(&path).unwrap();
}

#[test]
fn rejects_junction_directory() {
    let temporary = tempfile::tempdir().unwrap();
    let target = temporary.path().join("target");
    directory(&target).unwrap();
    let junction = temporary.path().join("junction");
    let output = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&junction)
        .arg(&target)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "junction creation failed: {output:?}"
    );
    assert!(Store::open(&junction).is_err());
    assert!(!target.join("node.lock").exists());
    assert!(!target.join("link.key").exists());
    std::fs::remove_dir(junction).unwrap();
}

#[test]
fn rejects_inherited_directory_access() {
    let temporary = tempfile::tempdir().unwrap();
    let parent = temporary.path().join("private");
    directory(&parent).unwrap();
    let path = parent.join("inherited");
    std::fs::create_dir(&path).unwrap();
    let before = descriptor(&path);
    assert!(Store::open(&path).is_err());
    assert_eq!(descriptor(&path), before);
    assert!(!path.join("node.lock").exists());
}

#[test]
fn rejects_null_access_list() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("unrestricted");
    create(&path, "D:NO_ACCESS_CONTROL");
    assert!(Store::open(&path).is_err());
    assert!(!path.join("node.lock").exists());
}

#[tokio::test]
async fn rejects_unsafe_replacement_target() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("profile");
    let (_, store) = Store::open(&path).unwrap();
    let peer = NodeId([1; 32]);
    store.set_trust(peer, true).await.unwrap();
    let file = path.join("peers.json");
    let before = std::fs::read(&file).unwrap();
    share(&file);
    let security = descriptor(&file);
    assert!(store.set_trust(NodeId([2; 32]), true).await.is_err());
    assert_eq!(descriptor(&file), security);
    assert_eq!(std::fs::read(file).unwrap(), before);
    assert_eq!(store.peers().await.unwrap(), vec![peer]);
    store.close().await.unwrap();
}

fn create(path: &Path, sddl: &str) {
    let security = sddl_descriptor(sddl);
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: security.0,
        bInheritHandle: 0,
    };
    let name = wide(path).unwrap();
    // SAFETY: both terminated name and descriptor remain live for creation.
    check(unsafe { CreateDirectoryW(name.as_ptr(), &attributes) }).unwrap();
}

fn share(path: &Path) {
    let security = sddl_descriptor("D:P(A;OICI;FA;;;WD)");
    let name = wide(path).unwrap();
    // SAFETY: the name and validated descriptor remain live for the call.
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
    let file = options().open(path).unwrap();
    let security = Security::read(&file).unwrap();
    // SAFETY: the self-relative descriptor and its entire allocation are live.
    unsafe {
        let length = GetSecurityDescriptorLength(security.allocation.0) as usize;
        std::slice::from_raw_parts(security.allocation.0.cast::<u8>(), length).to_vec()
    }
}
