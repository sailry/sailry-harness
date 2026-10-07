use super::*;
use serde_json::json;

fn write(path: &Path, files: &BTreeMap<String, Vec<u8>>, prefix: &str) {
    let file = std::fs::File::create(path).unwrap();
    let mut archive = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    if !prefix.is_empty() {
        archive.add_directory(prefix, options).unwrap();
    }
    archive
        .add_directory(format!("{prefix}assets/"), options)
        .unwrap();
    for (name, bytes) in files {
        archive
            .start_file(format!("{prefix}{name}"), options)
            .unwrap();
        archive.write_all(bytes).unwrap();
    }
    archive.finish().unwrap();
}

fn offsets(bytes: &[u8], name: &str) -> (usize, usize) {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let file = archive.by_name(name).unwrap();
    (
        file.header_start() as usize,
        file.central_header_start() as usize,
    )
}

#[test]
fn roundtrip_preserves_verified_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let mut value = crate::theme::fixture::manifest("tide");
    value["homepage"] = json!("https://example.invalid/themes/tide");
    value["light"]["assets"]["brand"]["license"] = json!({
        "author": "Example artist", "name": "Example license", "text": "Original light artwork license"
    });
    std::fs::write(
        source.join("theme.json"),
        serde_json::to_vec_pretty(&value).unwrap(),
    )
    .unwrap();
    let loaded = Loaded::read(&source, true).unwrap();
    let original = loaded.files.clone();
    std::fs::write(source.join("theme.json"), b"changed after preview").unwrap();
    std::fs::write(source.join("assets/light.png"), b"changed after preview").unwrap();
    let path = directory.path().join("tide.zip");
    std::fs::write(&path, b"previous export").unwrap();
    loaded.export(&path).unwrap();
    let archive = ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let names = archive
        .file_names()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(names, original.keys().map(String::as_str).collect());
    assert!(names.contains("theme.json"));
    drop(names);
    drop(archive);
    let restored = Loaded::read(&path, true).unwrap();
    assert_eq!(restored.files, original);
    assert_eq!(restored.images.len(), 2);
    assert_eq!(restored.manifest.version, 1);
    assert_eq!(restored.manifest.homepage, loaded.manifest.homepage);
    assert_eq!(restored.manifest.license.text, loaded.manifest.license.text);
    assert_eq!(restored.manifest.license.name, loaded.manifest.license.name);
    assert_eq!(
        restored.manifest.light.theme.mode,
        loaded.manifest.light.theme.mode
    );
    assert_eq!(
        restored.manifest.dark.theme.mode,
        loaded.manifest.dark.theme.mode
    );
    assert_eq!(
        restored.manifest.light.assets["brand"]
            .license
            .as_ref()
            .unwrap()
            .text,
        "Original light artwork license"
    );
    std::fs::write(&path, b"changed after preview").unwrap();
    let installed = directory.path().join("installed");
    restored.install(&installed).unwrap();
    let restored = Loaded::read(&installed.join("tide"), true).unwrap();
    assert_eq!(restored.files, original);
}

#[test]
fn accepts_root_and_containing_folder() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let loaded = Loaded::read(&source, true).unwrap();
    let mut files = loaded.files.clone();
    files.insert("README.md".into(), b"Package details".to_vec());
    files.insert("assets/unused.png".into(), b"Unreferenced data".to_vec());
    for prefix in ["", "tide/"] {
        let path = directory.path().join("package.zip");
        write(&path, &files, prefix);
        let restored = Loaded::read(&path, true).unwrap();
        assert_eq!(restored.files, loaded.files);
        assert_eq!(restored.images.len(), 2);
        assert!(restored.warnings.is_empty());
    }
}

#[test]
fn rejects_unsafe_paths_and_links() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let loaded = Loaded::read(&source, true).unwrap();
    let path = directory.path().join("package.zip");
    let outside = directory.path().join("outside.txt");
    std::fs::write(&outside, b"Preserved content").unwrap();
    for name in [
        "../outside.txt",
        "/outside.txt",
        "assets/../outside.png",
        "assets\\outside.png",
        "C:/outside.txt",
        "assets//unused.png",
        "assets/./unused.png",
    ] {
        let mut files = loaded.files.clone();
        files.insert(name.into(), Vec::new());
        write(&path, &files, "");
        for strict in [false, true] {
            assert!(
                Loaded::read(&path, strict).is_err(),
                "accepted unsafe path {name}"
            );
        }
    }
    let mut archive = ZipWriter::new(std::fs::File::create(&path).unwrap());
    for (name, bytes) in &loaded.files {
        archive
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        archive.write_all(bytes).unwrap();
    }
    archive
        .add_symlink(
            "assets/link.png",
            "../outside.txt",
            SimpleFileOptions::default(),
        )
        .unwrap();
    archive.finish().unwrap();
    for strict in [false, true] {
        assert!(Loaded::read(&path, strict).is_err());
    }
    assert_eq!(std::fs::read(outside).unwrap(), b"Preserved content");
}

#[test]
fn rejects_duplicate_entries() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let loaded = Loaded::read(&source, true).unwrap();
    let path = directory.path().join("package.zip");
    let mut files = loaded.files.clone();
    files.insert("other.json".into(), b"Duplicate manifest".to_vec());
    write(&path, &files, "");
    let mut bytes = std::fs::read(&path).unwrap();
    let offsets = bytes
        .windows(b"other.json".len())
        .enumerate()
        .filter_map(|(index, name)| (name == b"other.json").then_some(index))
        .collect::<Vec<_>>();
    assert_eq!(offsets.len(), 2);
    for offset in offsets {
        bytes[offset..offset + b"theme.json".len()].copy_from_slice(b"theme.json");
    }
    std::fs::write(&path, bytes).unwrap();
    for strict in [false, true] {
        let error = Loaded::read(&path, strict).err().unwrap();
        assert!(error.contains("duplicate"), "unexpected rejection: {error}");
    }
}

#[test]
fn rejects_corruption_and_encryption() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let loaded = Loaded::read(&source, true).unwrap();
    let path = directory.path().join("package.zip");
    write(&path, &loaded.files, "");
    let original = std::fs::read(&path).unwrap();
    let (local, central) = offsets(&original, "assets/light.png");
    let mut bytes = original.clone();
    bytes[central + 16] ^= 0xff;
    std::fs::write(&path, bytes).unwrap();
    for strict in [false, true] {
        let error = Loaded::read(&path, strict).err().unwrap();
        assert!(
            error.to_lowercase().contains("checksum"),
            "unexpected rejection: {error}"
        );
    }
    for header in [local + 6, central + 8] {
        let mut bytes = original.clone();
        bytes[header] |= 1;
        std::fs::write(&path, bytes).unwrap();
        assert!(Loaded::read(&path, true).is_err());
    }
    let mut bytes = original.clone();
    bytes[local + 30] = b'x';
    std::fs::write(&path, bytes).unwrap();
    assert!(
        Loaded::read(&path, true)
            .err()
            .unwrap()
            .contains("names do not match")
    );
    for bytes in [b"not a ZIP".as_slice(), &original[..original.len() - 22]] {
        std::fs::write(&path, bytes).unwrap();
        assert!(Loaded::read(&path, true).is_err());
    }
}

#[test]
fn rejects_missing_and_ambiguous_resources() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let loaded = Loaded::read(&source, true).unwrap();
    let path = directory.path().join("package.zip");
    for name in ["theme.json", "assets/light.png"] {
        let mut files = loaded.files.clone();
        files.remove(name);
        write(&path, &files, "");
        for strict in [false, true] {
            assert!(Loaded::read(&path, strict).is_err());
        }
    }
    let mut files = loaded.files.clone();
    files.insert("second/theme.json".into(), files["theme.json"].clone());
    write(&path, &files, "");
    assert!(Loaded::read(&path, true).is_err());
    write(&path, &loaded.files, "outer/inner/");
    assert!(Loaded::read(&path, true).is_err());
    write(&path, &loaded.files, "");
    let mut bytes = std::fs::read(&path).unwrap();
    let (_, central) = offsets(&bytes, "assets/light.png");
    bytes[central + 24..central + 28].copy_from_slice(&1u32.to_le_bytes());
    std::fs::write(&path, bytes).unwrap();
    assert!(Loaded::read(&path, true).is_err());
}

#[test]
fn bounds_encoded_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let loaded = Loaded::read(&source, true).unwrap();
    let path = directory.path().join("package.zip");
    std::fs::File::create(&path)
        .unwrap()
        .set_len(INPUT_LIMIT as u64 + 1)
        .unwrap();
    assert!(Loaded::read(&path, true).is_err());
    for (name, limit) in [
        ("theme.json", resources::MANIFEST_LIMIT),
        ("README.md", resources::FILE_LIMIT),
    ] {
        let mut files = loaded.files.clone();
        files.insert(name.into(), vec![0; limit + 1]);
        write(&path, &files, "");
        assert!(Loaded::read(&path, true).is_err());
    }
    let mut files = loaded.files.clone();
    for index in 0..5 {
        files.insert(format!("unused-{index}.bin"), vec![0; 7 * 1024 * 1024]);
    }
    write(&path, &files, "");
    assert!(Loaded::read(&path, true).is_err());
}

#[test]
fn near_limit_roundtrip() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "boundary");
    let original = std::fs::read(source.join("assets/light.png")).unwrap();
    let mut value = crate::theme::fixture::manifest("boundary");
    let names = ["a", "b", "c", "d"];
    value["light"]["assets"] = [
        "background.window",
        "background.content",
        "background.sidebar",
        "brand",
    ]
    .into_iter()
    .zip(names)
    .map(|(slot, name)| {
        (
            slot.to_string(),
            json!({"path": format!("assets/{name}.png")}),
        )
    })
    .collect::<serde_json::Map<_, _>>()
    .into();
    value["dark"]["assets"] = json!({});
    let manifest = serde_json::to_vec(&value).unwrap();
    std::fs::write(source.join("theme.json"), &manifest).unwrap();
    let mut noise = 0x721b_82d3u32;
    for (index, name) in names.into_iter().enumerate() {
        let length = resources::FILE_LIMIT - if index == 3 { manifest.len() } else { 0 };
        let mut bytes = original.clone();
        bytes.resize(length, 0);
        // The decoder keeps the original PNG; incompressible trailing bytes exercise ZIP overhead.
        for byte in &mut bytes[original.len()..] {
            noise ^= noise << 13;
            noise ^= noise >> 17;
            noise ^= noise << 5;
            *byte = noise as u8;
        }
        std::fs::write(source.join(format!("assets/{name}.png")), bytes).unwrap();
    }
    let loaded = Loaded::read(&source, true).unwrap();
    assert_eq!(
        loaded.files.values().map(Vec::len).sum::<usize>(),
        resources::PACKAGE_LIMIT
    );
    let path = directory.path().join("boundary.zip");
    loaded.export(&path).unwrap();
    let length = std::fs::metadata(&path).unwrap().len();
    assert!(length > resources::PACKAGE_LIMIT as u64);
    assert!(length <= INPUT_LIMIT as u64);
    let restored = Loaded::read(&path, true).unwrap();
    assert_eq!(restored.files, loaded.files);
    assert_eq!(restored.images.len(), 4);
}

#[test]
fn bounds_entry_count_and_paths() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let loaded = Loaded::read(&source, true).unwrap();
    let path = directory.path().join("package.zip");
    let mut files = loaded.files.clone();
    // The helper emits one assets directory in addition to these file entries.
    for index in 0..ENTRY_LIMIT - files.len() - 1 {
        files.insert(format!("empty/{index}.txt"), Vec::new());
    }
    write(&path, &files, "");
    assert!(Loaded::read(&path, true).is_ok());
    files.insert("extra.txt".into(), Vec::new());
    write(&path, &files, "");
    assert!(
        Loaded::read(&path, true)
            .err()
            .unwrap()
            .contains("entry limit")
    );
    for length in [512, 513] {
        let mut files = loaded.files.clone();
        files.insert("x".repeat(length), Vec::new());
        write(&path, &files, "");
        assert_eq!(Loaded::read(&path, true).is_ok(), length == 512);
    }
}

#[test]
fn bounds_referenced_images() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let loaded = Loaded::read(&source, true).unwrap();
    let path = directory.path().join("package.zip");
    let mut files = loaded.files.clone();
    let mut value = crate::theme::fixture::manifest("tide");
    let mut assets = serde_json::Map::new();
    for (index, slot) in SLOTS.iter().take(33).enumerate() {
        let path = format!("assets/{index}.png");
        files.insert(path.clone(), loaded.files["assets/light.png"].clone());
        assets.insert((*slot).into(), json!({"path": path}));
    }
    value["light"]["assets"] = assets.into();
    files.insert("theme.json".into(), serde_json::to_vec(&value).unwrap());
    write(&path, &files, "");
    assert!(Loaded::read(&path, true).is_err());
}

#[test]
fn failure_preserves_existing_destination() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let path = directory.path().join("package.zip");
    std::fs::write(&path, b"Existing export").unwrap();
    std::fs::remove_file(source.join("assets/light.png")).unwrap();
    let loaded = Loaded::read(&source, false).unwrap();
    assert!(!loaded.warnings.is_empty());
    assert!(loaded.export(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"Existing export");
    let source = crate::theme::fixture::write(directory.path(), "second");
    let loaded = Loaded::read(&source, true).unwrap();
    let destination = directory.path().join("existing");
    std::fs::create_dir(&destination).unwrap();
    std::fs::write(destination.join("preserved.txt"), b"Preserved content").unwrap();
    assert!(loaded.export(&destination).is_err());
    assert_eq!(
        std::fs::read(destination.join("preserved.txt")).unwrap(),
        b"Preserved content"
    );
}

mod homepage {
    use super::*;

    #[test]
    fn allows_omission_and_null() {
        for null in [false, true] {
            let mut value = crate::theme::fixture::manifest("tide");
            if null {
                value["homepage"] = serde_json::Value::Null;
            } else {
                value.as_object_mut().unwrap().remove("homepage");
            }
            let manifest: Manifest = serde_json::from_value(value).unwrap();
            manifest.validate().unwrap();
            assert!(manifest.homepage.is_none());
            assert!(
                serde_json::to_value(manifest)
                    .unwrap()
                    .get("homepage")
                    .is_none()
            );
        }
    }

    #[test]
    fn accepts_http_and_https() {
        for homepage in [
            "https://example.invalid/themes/tide?release=1#license",
            "http://example.invalid/",
            "http://localhost:3000/",
            "http://127.0.0.1:3000/",
        ] {
            let mut value = crate::theme::fixture::manifest("tide");
            value["homepage"] = json!(homepage);
            let manifest: Manifest = serde_json::from_value(value).unwrap();
            manifest.validate().unwrap();
            assert_eq!(manifest.homepage.as_deref(), Some(homepage));
        }
    }

    #[test]
    fn rejects_invalid_urls() {
        for homepage in [
            "",
            " ",
            "/themes/tide",
            "https://",
            "https://[:::1]",
            "https://user@example.invalid/",
            "https://:secret@example.invalid/",
            "https://example.invalid/path with spaces",
            " https://example.invalid/",
            "https://example.invalid/\n",
            "https://example.invalid/\0",
            "file:///tmp/theme",
            "ftp://example.invalid/",
            "javascript:alert(1)",
        ] {
            let mut value = crate::theme::fixture::manifest("tide");
            value["homepage"] = json!(homepage);
            let manifest: Manifest = serde_json::from_value(value).unwrap();
            assert!(
                manifest.validate().is_err(),
                "accepted invalid URL {homepage}"
            );
        }
    }
}
