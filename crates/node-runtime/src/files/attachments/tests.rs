use super::*;
use sailry_protocol::WorktreeId;

fn spec() -> attachment::Spec {
    attachment::Spec {
        worktree: WorktreeId::new(),
        name: "说明 🙂.txt".into(),
        media_type: "text/plain".into(),
        size: 0,
        revision: blake3::hash(&[]).to_hex().to_string(),
    }
}

#[tokio::test]
async fn waits_for_capacity_and_cancels() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().canonicalize().unwrap();
    let (attachment, target) = prepare(Some(&profile), spec()).unwrap();
    target
        .write(&b""[..], 0, &attachment.spec.revision, &|_| {})
        .unwrap();
    let files = super::super::Files::new();
    let permit = files.capacity.clone().acquire_many_owned(4).await.unwrap();
    let stop = sailry_link::CancellationToken::new();
    let read = files.attachment(Some(profile.clone()), attachment.clone(), stop.clone());
    tokio::pin!(read);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(10), &mut read)
            .await
            .is_err()
    );
    drop(permit);
    assert!(read.await.unwrap().is_empty());
    let _permit = files.capacity.clone().acquire_many_owned(4).await.unwrap();
    stop.cancel();
    assert_eq!(
        files
            .attachment(Some(profile), attachment, stop)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
}

#[test]
fn checks_metadata() {
    let valid = spec();
    validate(&valid).unwrap();
    for name in ["", "../file", "a/b", "a\\b", "a\0b", "a\nb"] {
        assert!(
            validate(&attachment::Spec {
                name: name.into(),
                ..valid.clone()
            })
            .is_err()
        );
    }
    for media_type in [
        "",
        "/png",
        "image/",
        "image/png/extra",
        "text/plain; charset=utf-8",
        "image/图",
    ] {
        assert!(
            validate(&attachment::Spec {
                media_type: media_type.into(),
                ..valid.clone()
            })
            .is_err()
        );
    }
    assert!(
        validate(&attachment::Spec {
            size: attachment::MAX_BYTES + 1,
            ..valid
        })
        .is_err()
    );
}

#[test]
fn retains_registered_files() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().canonicalize().unwrap();
    let (attachment, target) = prepare(Some(&profile), spec()).unwrap();
    target
        .write(&b""[..], 0, &attachment.spec.revision, &|_| {})
        .unwrap();
    let file = profile.join("attachments").join(attachment.id.to_string());
    let orphan = profile
        .join("attachments")
        .join(AttachmentId::new().to_string());
    std::fs::write(&orphan, b"unfinished publication").unwrap();
    let unknown = profile.join("attachments/notes");
    std::fs::write(&unknown, b"keep").unwrap();
    let folder = profile
        .join("attachments")
        .join(AttachmentId::new().to_string());
    std::fs::create_dir(&folder).unwrap();
    collect(&profile, &BTreeSet::from([attachment.id])).unwrap();
    assert!(file.exists());
    assert!(!orphan.exists());
    assert!(unknown.exists());
    assert!(folder.exists());
    assert_eq!(
        open(Some(&profile), &attachment)
            .unwrap()
            .metadata()
            .unwrap()
            .len(),
        0
    );
    collect(&profile, &BTreeSet::new()).unwrap();
    assert!(!file.exists());
}

#[test]
fn leaves_missing_storage() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().canonicalize().unwrap();
    collect(&profile, &BTreeSet::new()).unwrap();
    assert!(!profile.join("attachments").exists());
    assert!(prepare(None, spec()).is_err());
}

#[cfg(unix)]
#[test]
fn keeps_symlinks() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().canonicalize().unwrap();
    std::fs::create_dir(profile.join("attachments")).unwrap();
    let outside = profile.join("outside");
    std::fs::write(&outside, b"keep").unwrap();
    let name = AttachmentId::new();
    let link = profile.join("attachments").join(name.to_string());
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    collect(&profile, &BTreeSet::new()).unwrap();
    assert!(link.symlink_metadata().unwrap().is_symlink());
    assert_eq!(std::fs::read(&outside).unwrap(), b"keep");
    assert!(
        open(
            Some(&profile),
            &attachment::Attachment {
                id: name,
                spec: spec()
            }
        )
        .is_err()
    );
    let other = tempfile::tempdir().unwrap();
    let other = other.path().canonicalize().unwrap();
    std::os::unix::fs::symlink(profile.join("attachments"), other.join("attachments")).unwrap();
    assert!(prepare(Some(&other), spec()).is_err());
    assert!(collect(&other, &BTreeSet::new()).is_err());
}
