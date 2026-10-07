use super::*;
use serde_json::json;

#[test]
fn validation_and_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let loaded = Loaded::read(&source, true).unwrap();
    let packages = directory.path().join("installed");
    // Installation uses the verified snapshot, including its original license.
    std::fs::write(source.join("assets/light.png"), "changed after preview").unwrap();
    loaded.install(&packages).unwrap();
    assert!(loaded.install(&packages).is_err());
    let installed = packages.join("tide");
    let restored = Loaded::read(&installed, true).unwrap();
    assert_eq!(restored.images.len(), 2);
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
fn great_sage_imports() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes/great-sage");
    let loaded = Loaded::read(&source, true).unwrap();
    assert_eq!(loaded.manifest.version, 1);
    assert_eq!(loaded.manifest.id, "great-sage");
    assert_eq!(loaded.images.len(), 5);
    for variant in [&loaded.manifest.light, &loaded.manifest.dark] {
        assert_eq!(variant.composer.border_width, 1);
        assert_eq!(variant.cards.len(), 4);
        for style in variant.cards.values() {
            assert_ne!(style.hover, style.background);
            assert_ne!(style.active, style.hover);
        }
        let image = &variant.assets["background.content"];
        assert!(matches!(image.fit, Fit::Cover));
        let decoded = image::open(source.join(&image.path)).unwrap().into_rgba8();
        assert!(decoded.pixels().any(|pixel| pixel[3] == 0));
        assert!(decoded.width() >= 1600);
    }
    assert!(
        loaded.manifest.light.assets["background.content"].opacity
            < loaded.manifest.dark.assets["background.content"].opacity
    );
    let directory = tempfile::tempdir().unwrap();
    loaded.install(directory.path()).unwrap();
    let installed = Loaded::read(&directory.path().join("great-sage"), true).unwrap();
    assert_eq!(installed.manifest.name, loaded.manifest.name);
    assert_eq!(installed.images.len(), loaded.images.len());
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
