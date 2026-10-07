use super::*;
use serde_json::json;

#[test]
fn validation_and_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let manifest = std::fs::read(source.join("theme.json")).unwrap();
    let loaded = Loaded::read(&source, true).unwrap();
    let packages = directory.path().join("installed");
    // Installation uses the verified snapshot, including its original license.
    std::fs::write(source.join("assets/light.png"), "changed after preview").unwrap();
    std::fs::write(source.join("theme.json"), "changed after preview").unwrap();
    loaded.install(&packages).unwrap();
    loaded.install(&packages).unwrap();
    let installed = packages.join("tide");
    assert_eq!(
        std::fs::read(installed.join("theme.json")).unwrap(),
        manifest
    );
    let restored = Loaded::read(&installed, true).unwrap();
    assert_eq!(restored.images.len(), 2);
    assert_eq!(restored.manifest.description, loaded.manifest.description);
    assert_eq!(restored.manifest.package_version.as_deref(), Some("0.1.0"));
    assert_eq!(restored.manifest.license.name.as_deref(), Some("CC0-1.0"));
    assert_eq!(restored.manifest.license.text, loaded.manifest.license.text);
    std::fs::remove_file(installed.join("assets/light.png")).unwrap();
    assert!(Loaded::read(&installed, true).is_err());
    let fallback = Loaded::read(&installed, false).unwrap();
    assert_eq!(fallback.images.len(), 1);
    assert!(!fallback.warnings.is_empty());
    assert_eq!(
        fallback.manifest.light.theme.colors.group_box.as_deref(),
        Some("#e0eef5")
    );
}

#[test]
fn rejects_invalid_packages() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let original = crate::theme::fixture::manifest("tide");
    for (pointer, replacement) in [
        ("/version", json!(2)),
        ("/id", json!("../outside")),
        ("/license/text", json!("")),
        ("/dark/theme/mode", json!("light")),
        ("/light/theme/colors/background", json!("not-a-color")),
        (
            "/light/theme/colors/background",
            json!("https://example.invalid/theme.png"),
        ),
        ("/light/assets/brand/path", json!("assets/../secret.png")),
        ("/light/assets/brand/path", json!("/tmp/secret.png")),
        ("/light/assets/brand/path", json!("assets\\secret.png")),
        (
            "/light/assets/brand/path",
            json!("https://example.invalid/image.png"),
        ),
        ("/light/assets/brand/path", json!("assets/script.svg")),
        ("/light/assets/brand/opacity", json!(2)),
        ("/light/assets/brand/height", json!(10000)),
    ] {
        let mut value = original.clone();
        *value.pointer_mut(pointer).unwrap() = replacement;
        std::fs::write(
            source.join("theme.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        assert!(
            Loaded::read(&source, true).is_err(),
            "accepted invalid field {pointer}"
        );
    }
}

#[test]
fn rejects_invalid_styles() {
    let mut value = crate::theme::fixture::manifest("tide");
    value["light"]["composer"] = json!({"border_width": 1});
    value["light"]["cards"] = json!({"explore": {
        "background": "#182b36", "border_width": 1, "radius": 16
    }});
    value["light"]["assets"]["background.content"]["fit"] = json!("tile");
    value["light"]["assets"]["background.content"]["tile_size"] = json!(128);
    let original: Manifest = serde_json::from_value(value.clone()).unwrap();
    original.validate().unwrap();
    for (pointer, replacement) in [
        ("/light/composer/border_width", json!(5)),
        ("/light/cards/explore/background", json!("not-a-color")),
        (
            "/light/cards/explore/background",
            json!("linear-gradient(black, red)"),
        ),
        ("/light/cards/explore/border_width", json!(5)),
        ("/light/cards/explore/radius", json!(33)),
        ("/light/cards", json!({"other": {}})),
        ("/light/assets/background.content/tile_size", json!(0)),
        ("/light/assets/background.content/tile_size", json!(1025)),
        ("/light/assets/icon.send/fit", json!("tile")),
    ] {
        let mut invalid = value.clone();
        *invalid.pointer_mut(pointer).unwrap() = replacement;
        let manifest: Manifest = serde_json::from_value(invalid).unwrap();
        assert!(
            manifest.validate().is_err(),
            "accepted invalid field {pointer}"
        );
    }
}

#[test]
fn bounds_files_and_decoding() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let path = source.join("assets/light.png");
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(8 * 1024 * 1024 + 1)
        .unwrap();
    assert!(Loaded::read(&source, true).is_err());
    image::RgbaImage::new(4097, 1).save(&path).unwrap();
    assert!(Loaded::read(&source, true).is_err());
    std::fs::write(&path, b"not an image").unwrap();
    assert!(Loaded::read(&source, true).is_err());
    std::fs::File::options()
        .write(true)
        .open(source.join("theme.json"))
        .unwrap()
        .set_len(256 * 1024 + 1)
        .unwrap();
    assert!(Loaded::read(&source, true).is_err());
}

#[cfg(unix)]
#[test]
fn rejects_symlinks_on_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let outside = directory.path().join("outside.png");
    std::fs::rename(source.join("assets/light.png"), &outside).unwrap();
    std::os::unix::fs::symlink(&outside, source.join("assets/light.png")).unwrap();
    assert!(Loaded::read(&source, true).is_err());
    assert!(Loaded::read(&source, false).is_err());
    std::fs::remove_file(source.join("assets/light.png")).unwrap();
    std::fs::rename(&outside, source.join("assets/light.png")).unwrap();
    std::fs::rename(
        source.join("assets"),
        directory.path().join("outside-assets"),
    )
    .unwrap();
    std::os::unix::fs::symlink(
        directory.path().join("outside-assets"),
        source.join("assets"),
    )
    .unwrap();
    assert!(Loaded::read(&source, true).is_err());
}

mod metadata {
    use super::*;

    #[test]
    fn allows_omission_and_null() {
        for null in [false, true] {
            let mut value = crate::theme::fixture::manifest("tide");
            for key in ["description", "package_version"] {
                if null {
                    value[key] = serde_json::Value::Null;
                } else {
                    value.as_object_mut().unwrap().remove(key);
                }
            }
            if null {
                value["license"]["name"] = serde_json::Value::Null;
            } else {
                value["license"].as_object_mut().unwrap().remove("name");
            }
            let manifest: Manifest = serde_json::from_value(value).unwrap();
            manifest.validate().unwrap();
            assert_eq!(manifest.version, 1);
            assert!(manifest.description.is_none());
            assert!(manifest.package_version.is_none());
            assert!(manifest.license.name.is_none());
            let encoded = serde_json::to_value(manifest).unwrap();
            assert!(encoded.get("description").is_none());
            assert!(encoded.get("package_version").is_none());
            assert!(encoded["license"].get("name").is_none());
        }
    }

    #[test]
    fn preserves_values() {
        let mut value = crate::theme::fixture::manifest("tide");
        value["description"] = json!("First paragraph.\n\nSecond paragraph.\tDetails");
        value["package_version"] = json!("2026.10-rc.1");
        value["license"]["name"] = json!("Example License");
        let manifest: Manifest = serde_json::from_value(value.clone()).unwrap();
        manifest.validate().unwrap();
        let encoded = serde_json::to_value(manifest).unwrap();
        for pointer in ["/description", "/package_version", "/license/name"] {
            assert_eq!(encoded.pointer(pointer), value.pointer(pointer));
        }
    }

    #[test]
    fn accepts_bounds() {
        let mut value = crate::theme::fixture::manifest("tide");
        value["description"] = json!("d".repeat(4 * 1024));
        value["package_version"] = json!("v".repeat(64));
        value["license"]["name"] = json!("l".repeat(160));
        let manifest: Manifest = serde_json::from_value(value).unwrap();
        manifest.validate().unwrap();
    }

    #[test]
    fn rejects_invalid_values() {
        let original = crate::theme::fixture::manifest("tide");
        for (pointer, replacement) in [
            ("/description", json!(" \n\t")),
            ("/description", json!("d".repeat(4 * 1024 + 1))),
            ("/description", json!("invalid\u{0}")),
            ("/package_version", json!(" ")),
            ("/package_version", json!("v".repeat(65))),
            ("/package_version", json!("0.1.0\n")),
            ("/license/name", json!(" ")),
            ("/license/name", json!("l".repeat(161))),
            ("/license/name", json!("License\t")),
        ] {
            let mut value = original.clone();
            *value.pointer_mut(pointer).unwrap() = replacement;
            let manifest: Manifest = serde_json::from_value(value).unwrap();
            assert!(
                manifest.validate().is_err(),
                "accepted invalid field {pointer}"
            );
        }
    }

    #[test]
    fn validates_asset_licenses() {
        let mut value = crate::theme::fixture::manifest("tide");
        value["light"]["assets"]["brand"]["license"] = value["license"].clone();
        let manifest: Manifest = serde_json::from_value(value.clone()).unwrap();
        manifest.validate().unwrap();
        value["light"]["assets"]["brand"]["license"]["name"] = json!(" ");
        let manifest: Manifest = serde_json::from_value(value).unwrap();
        assert!(manifest.validate().is_err());
    }
}

mod message_styles {
    use super::*;

    #[test]
    fn validates_colors_and_dimensions() {
        let mut value = crate::theme::fixture::manifest("styles");
        value["light"]["messages"] = json!({"user": {
            "background": "#231C16CC", "foreground": "#FFEBCD", "border": "#B9954150",
            "border_width": 1, "radius": 14
        }});
        value["light"]["wordmark"] = json!({"foreground": "#A47C26", "glow": "#BE7331"});
        serde_json::from_value::<Manifest>(value.clone())
            .unwrap()
            .validate()
            .unwrap();
        for (pointer, replacement) in [
            ("/light/messages/user/background", json!("invalid")),
            ("/light/messages/user/foreground", json!("invalid")),
            ("/light/messages/user/border", json!("invalid")),
            ("/light/messages/user/border_width", json!(5)),
            ("/light/messages/user/radius", json!(33)),
            ("/light/messages", json!({"system": {}})),
            ("/light/wordmark/glow", json!("invalid")),
            ("/light/wordmark/foreground", json!("invalid")),
        ] {
            let mut invalid = value.clone();
            *invalid.pointer_mut(pointer).unwrap() = replacement;
            assert!(
                serde_json::from_value::<Manifest>(invalid)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
    }
}
