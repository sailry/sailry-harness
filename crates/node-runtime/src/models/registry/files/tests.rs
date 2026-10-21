use super::*;
use crate::models::test_support::record;

fn seed(id: &str) -> (tempfile::TempDir, Definition, PathBuf) {
    let profile = tempfile::tempdir().unwrap();
    let (mut record, contents) = record(2, &[100., 0., 1., 0., 0., 1.]);
    record.definition.assets.reverse();
    let path = directory(&root(profile.path()).unwrap(), id, &record.definition).unwrap();
    std::fs::create_dir_all(&path).unwrap();
    for (name, body) in contents {
        std::fs::write(path.join(name), body).unwrap();
    }
    (profile, record.definition, path)
}

fn previous(definition: &Definition) -> String {
    blake3::hash(&serde_json::to_vec(definition).unwrap())
        .to_hex()
        .to_string()
}

#[test]
fn normalizes_existing_validation_markers() {
    for id in ["granite-multi", "model-imported"] {
        let (_profile, definition, path) = seed(id);
        let files = assets(&definition);
        if id.starts_with("model-") {
            let marker = previous(&definition);
            assert_ne!(marker, catalog::id(&definition));
            verified::write_marker(&crate::files::path::root(&path).unwrap(), &marker).unwrap();
        }
        let before = verified::candidate(&path, &files).unwrap();
        assert_eq!(stamp(&path, id, &definition, None), Some(before.clone()));
        assert_eq!(
            std::fs::read_to_string(path.join("validated")).unwrap(),
            catalog::id(&definition)
        );
        assert_eq!(
            verified::verified(&path, &files, Some(&catalog::id(&definition))),
            Some(before)
        );
    }
}

#[cfg(unix)]
#[test]
fn preserves_read_only_installed_models() {
    use std::os::unix::fs::PermissionsExt;
    let (_profile, definition, path) = seed("model-imported");
    let marker = previous(&definition);
    verified::write_marker(&crate::files::path::root(&path).unwrap(), &marker).unwrap();
    let before = verified::candidate(&path, &assets(&definition)).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o500)).unwrap();
    let observed = stamp(&path, "model-imported", &definition, None);
    let stored = std::fs::read_to_string(path.join("validated")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(observed, Some(before));
    // Root can write a read-only directory; ordinary owners retain the old marker.
    // SAFETY: geteuid reads process metadata and takes no pointers.
    if unsafe { libc::geteuid() } != 0 {
        assert_eq!(stored, marker);
    }
}

#[test]
fn refuses_unverified_imported_assets() {
    let (_profile, definition, path) = seed("model-imported");
    verified::write_marker(
        &crate::files::path::root(&path).unwrap(),
        &previous(&definition),
    )
    .unwrap();
    let file = path.join("model.safetensors");
    let mut body = std::fs::read(&file).unwrap();
    let last = body.last_mut().unwrap();
    *last ^= 1;
    std::fs::write(file, body).unwrap();
    assert!(stamp(&path, "model-imported", &definition, None).is_none());
    assert_eq!(
        std::fs::read_to_string(path.join("validated")).unwrap(),
        previous(&definition)
    );
}
