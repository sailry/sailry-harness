use super::super::{
    config::Config,
    manifest::{Release, Selection},
    transfer::Staged,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::Signer as _;
use std::{
    fs,
    io::{Cursor, Write},
    path::Path,
};

pub(in crate::updater) fn config() -> Config {
    Config {
        source: "https://example.test/update-v1.json".into(),
        keys: vec![
            ed25519_dalek::SigningKey::from_bytes(&[7; 32])
                .verifying_key()
                .to_bytes(),
        ],
        target: self_update::get_target().into(),
        version: "0.1.0".into(),
        system: "99.0.0".into(),
    }
}

pub(super) fn release() -> Release {
    let config = config();
    let (bundle, executable) = super::super::manifest::layout(&config.target).unwrap();
    Release {
        version: "9.9.9".into(),
        target: config.target.clone(),
        minimum_system: "13.0".into(),
        name: format!("Sailry-9.9.9-{}.zip", config.target),
        url: "https://example.test/package.zip".into(),
        sha256: "a".repeat(64),
        size: 1,
        bundle: bundle.into(),
        executable: executable.into(),
    }
}

pub(in crate::updater) fn signed(releases: &[Release]) -> Vec<u8> {
    let payload = serde_json::to_string(&serde_json::json!({"releases":releases})).unwrap();
    let signature = ed25519_dalek::SigningKey::from_bytes(&[7; 32]).sign(payload.as_bytes());
    format!(
        "{{\"version\":1,\"payload\":{payload},\"signature\":\"{}\"}}",
        STANDARD.encode(signature.to_bytes())
    )
    .into_bytes()
}

pub(super) fn archive(entries: &[(&str, &[u8])], name: &str) -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes) in entries {
        archive
            .start_file(
                *path,
                zip::write::SimpleFileOptions::default().unix_permissions(0o755),
            )
            .unwrap();
        archive.write_all(bytes).unwrap();
    }
    sign_archive(archive.finish().unwrap().into_inner(), name)
}

pub(super) fn sign_archive(bytes: Vec<u8>, name: &str) -> Vec<u8> {
    let mut result = Cursor::new(Vec::new());
    zipsign_api::sign::copy_and_sign_zip(
        &mut Cursor::new(bytes),
        &mut result,
        &[zipsign_api::SigningKey::from_bytes(&[7; 32])],
        Some(name.as_bytes()),
    )
    .unwrap();
    result.into_inner()
}

pub(super) fn digest(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    format!("{:x}", sha2::Sha256::digest(bytes))
}

#[cfg(target_os = "macos")]
pub(super) fn bundle(root: &Path, release: &Release) {
    let resources = root.join("Contents/Resources");
    fs::create_dir_all(root.join("Contents/MacOS")).unwrap();
    fs::create_dir_all(&resources).unwrap();
    fs::write(resources.join("build.json"), serde_json::to_vec(&serde_json::json!({"version":1,"target":release.target,"application_version":release.version})).unwrap()).unwrap();
    fs::write(root.join("Contents/Info.plist"), format!("<?xml version=\"1.0\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>ai.sailry.desktop</string><key>CFBundleExecutable</key><string>sailry-desktop</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleShortVersionString</key><string>{}</string><key>LSMinimumSystemVersion</key><string>{}</string></dict></plist>", release.version, release.minimum_system)).unwrap();
    // Build and ad hoc sign only isolated fixtures; neither fixture is executed.
    let source = root.parent().unwrap().join("fixture.c");
    fs::write(&source, "int main(void) { return 0; }\n").unwrap();
    let binary = root.join(&release.executable);
    assert!(
        std::process::Command::new("/usr/bin/cc")
            .arg(&source)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap()
            .status
            .success()
    );
    for target in [&binary, root] {
        let output = std::process::Command::new("/usr/bin/codesign")
            .args(["--force", "--sign", "-"])
            .arg(target)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[cfg(target_os = "macos")]
pub(in crate::updater) fn staged() -> Staged {
    let directory = tempfile::tempdir().unwrap();
    let mut release = release();
    let root = directory.path().join("Sailry.app");
    bundle(&root, &release);
    let unsigned = directory.path().join("unsigned.zip");
    assert!(
        std::process::Command::new("/usr/bin/ditto")
            .args(["-c", "-k", "--norsrc", "--keepParent"])
            .arg(&root)
            .arg(&unsigned)
            .output()
            .unwrap()
            .status
            .success()
    );
    let bytes = sign_archive(fs::read(&unsigned).unwrap(), &release.name);
    let archive = directory.path().join(&release.name);
    fs::write(&archive, &bytes).unwrap();
    release.sha256 = digest(&bytes);
    release.size = bytes.len() as u64;
    let manifest = signed(&[release.clone()]);
    Staged {
        directory,
        archive,
        selection: Selection { manifest, release },
    }
}
