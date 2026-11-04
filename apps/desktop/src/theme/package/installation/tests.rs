use super::*;

#[test]
fn publication_recovery() {
    for (scenario, check) in [
        (
            "replaces_verified_snapshot_and_removes_old_resources",
            (|_scenario: &str| {
                let directory = tempfile::tempdir().unwrap();
                let source = crate::theme::fixture::write(directory.path(), "tide");
                let root = directory.path().join("installed");
                Loaded::read(&source, true).unwrap().install(&root).unwrap();
                let installed = root.join("tide");
                fs::write(installed.join("assets/obsolete.png"), b"obsolete").unwrap();
                let mut manifest = crate::theme::fixture::manifest("tide");
                manifest["name"] = serde_json::json!("Updated Tide");
                manifest["package_version"] = serde_json::json!("0.2.0");
                manifest["license"]["text"] = serde_json::json!("Updated license");
                for asset in manifest["light"]["assets"]
                    .as_object_mut()
                    .unwrap()
                    .values_mut()
                {
                    asset["path"] = serde_json::json!("assets/replacement.png");
                }
                fs::rename(
                    source.join("assets/light.png"),
                    source.join("assets/replacement.png"),
                )
                .unwrap();
                let bytes = serde_json::to_vec(&manifest).unwrap();
                fs::write(source.join("theme.json"), &bytes).unwrap();
                let loaded = Loaded::read(&source, true).unwrap();
                fs::write(source.join("theme.json"), b"changed after preview").unwrap();
                loaded.install(&root).unwrap();
                assert_eq!(
                    fs::read(installed.join("theme.json")).unwrap(),
                    bytes,
                    "replaces verified snapshot and removes old resources"
                );
                assert!(
                    !installed.join("assets/light.png").exists(),
                    "replaces verified snapshot and removes old resources"
                );
                assert!(
                    !installed.join("assets/obsolete.png").exists(),
                    "replaces verified snapshot and removes old resources"
                );
                let restored = Loaded::read(&installed, true).unwrap();
                assert_eq!(
                    restored.manifest.name, "Updated Tide",
                    "replaces verified snapshot and removes old resources"
                );
                assert_eq!(
                    restored.manifest.package_version.as_deref(),
                    Some("0.2.0"),
                    "replaces verified snapshot and removes old resources"
                );
                assert_eq!(
                    restored.manifest.license.text, "Updated license",
                    "replaces verified snapshot and removes old resources"
                );
                assert!(
                    restored.images.contains_key("assets/replacement.png"),
                    "replaces verified snapshot and removes old resources"
                );
                assert_eq!(
                    fs::read_dir(root).unwrap().count(),
                    1,
                    "replaces verified snapshot and removes old resources"
                );
            }) as fn(&str),
        ),
        (
            "failed_staging_preserves_previous_package",
            (|_scenario: &str| {
                let directory = tempfile::tempdir().unwrap();
                let source = crate::theme::fixture::write(directory.path(), "tide");
                let root = directory.path().join("installed");
                let mut loaded = Loaded::read(&source, true).unwrap();
                loaded.install(&root).unwrap();
                let original = fs::read(root.join("tide/theme.json")).unwrap();
                // Force a write failure inside staging, after writing an ordinary resource.
                loaded
                    .files
                    .insert("assets/light.png/nested.png".into(), vec![0]);
                let error = loaded.install(&root).unwrap_err();
                assert!(
                    error.recovery.is_none(),
                    "failed staging preserves previous package"
                );
                assert_eq!(
                    fs::read(root.join("tide/theme.json")).unwrap(),
                    original,
                    "failed staging preserves previous package"
                );
                assert_eq!(
                    Loaded::read(&root.join("tide"), true).unwrap().images.len(),
                    2,
                    "failed staging preserves previous package"
                );
                assert_eq!(
                    fs::read_dir(root).unwrap().count(),
                    1,
                    "failed staging preserves previous package"
                );
            }),
        ),
        (
            "failed_publication_restores_previous_package",
            (|_scenario: &str| {
                let directory = tempfile::tempdir().unwrap();
                let destination = crate::theme::fixture::write(directory.path(), "tide");
                let original = fs::read(destination.join("theme.json")).unwrap();
                let error = publish(&directory.path().join("missing"), &destination).unwrap_err();
                assert!(
                    error.recovery.is_none(),
                    "failed publication restores previous package"
                );
                assert_eq!(
                    fs::read(destination.join("theme.json")).unwrap(),
                    original,
                    "failed publication restores previous package"
                );
                assert_eq!(
                    Loaded::read(&destination, true).unwrap().images.len(),
                    2,
                    "failed publication restores previous package"
                );
                assert_eq!(
                    fs::read_dir(directory.path()).unwrap().count(),
                    1,
                    "failed publication restores previous package"
                );
            }),
        ),
        (
            "refuses_changed_destination",
            (|_scenario: &str| {
                let directory = tempfile::tempdir().unwrap();
                let source = crate::theme::fixture::write(directory.path(), "tide");
                let loaded = Loaded::read(&source, true).unwrap();
                let root = directory.path().join("installed");
                fs::create_dir(&root).unwrap();
                let destination = root.join("tide");
                fs::write(&destination, b"not a theme directory").unwrap();
                assert!(
                    loaded.install(&root).is_err(),
                    "refuses changed destination"
                );
                assert_eq!(
                    fs::read(destination).unwrap(),
                    b"not a theme directory",
                    "refuses changed destination"
                );
                assert_eq!(
                    fs::read_dir(root).unwrap().count(),
                    1,
                    "refuses changed destination"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}

#[cfg(unix)]
#[test]
fn refuses_linked_destination() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let loaded = Loaded::read(&source, true).unwrap();
    let root = directory.path().join("installed");
    fs::create_dir(&root).unwrap();
    std::os::unix::fs::symlink(&source, root.join("tide")).unwrap();
    assert!(loaded.install(&root).is_err());
    assert!(
        fs::symlink_metadata(root.join("tide"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(Loaded::read(&source, true).unwrap().manifest.name, "Tide");
}
