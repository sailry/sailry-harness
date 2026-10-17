use super::{Channel, Result, archive, feed, key};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::SigningKey;
use std::{
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
};

const REPOSITORY: &str = "sailry/sailry-harness";

fn signing_key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn ring(signer: &SigningKey) -> Vec<[u8; 32]> {
    vec![signer.verifying_key().to_bytes()]
}

fn unsigned(path: &Path, contents: &[u8]) {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    archive
        .start_file(
            "Sailry.app/fixture",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    archive.write_all(contents).unwrap();
    fs::write(path, archive.finish().unwrap().into_inner()).unwrap();
}

fn signed(directory: &Path, target: &str, signer: &SigningKey) -> PathBuf {
    let name = format!("Sailry-{target}.zip");
    let source = directory.join(format!("{target}.unsigned.zip"));
    let output = directory.join(&name);
    unsigned(&source, b"isolated updater fixture");
    archive::sign(&source, &output, &name, signer, &ring(signer)).unwrap();
    fs::remove_file(source).unwrap();
    output
}

fn release(path: &Path, version: &str, target: &str, signer: &SigningKey) -> feed::Release {
    let ring = ring(signer);
    let archive = signed(path, target, signer);
    feed::describe(
        &archive,
        version,
        target,
        "13.0",
        REPOSITORY,
        &format!("v{version}"),
        &ring,
    )
    .unwrap()
}

fn assets(path: &Path, version: &str, signer: &SigningKey) -> Vec<feed::Release> {
    let mut releases = Vec::new();
    for target in ["aarch64-apple-darwin", "x86_64-apple-darwin"] {
        let release = release(path, version, target, signer);
        fs::write(
            path.join(format!("{}.update.json", release.name)),
            serde_json::to_vec(&release).unwrap(),
        )
        .unwrap();
        releases.push(release);
    }
    releases
}

mod archive_signing {
    use super::*;

    #[test]
    fn binds_name_and_rejects_tampering() {
        let directory = tempfile::tempdir().unwrap();
        let signer = signing_key(7);
        let path = signed(directory.path(), "aarch64-apple-darwin", &signer);
        let name = path.file_name().unwrap().to_str().unwrap();
        archive::verify(&path, name, &ring(&signer)).unwrap();
        assert!(archive::verify(&path, "Sailry-other.zip", &ring(&signer)).is_err());
        assert!(archive::verify(&path, name, &ring(&signing_key(8))).is_err());
        let mut bytes = fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        fs::write(&path, bytes).unwrap();
        assert!(archive::verify(&path, name, &ring(&signer)).is_err());
    }
}

mod publisher_keys {
    use super::*;

    #[test]
    fn requires_membership_in_ring() {
        let trusted = signing_key(7);
        let encoded = STANDARD.encode([7; 32]);
        assert_eq!(
            key::signer(&encoded, &ring(&trusted))
                .unwrap()
                .verifying_key(),
            trusted.verifying_key()
        );
        assert!(key::signer(&STANDARD.encode([8; 32]), &ring(&trusted)).is_err());
    }
}

mod feed_authentication {
    use super::*;

    #[test]
    fn signs_exact_payload_and_rejects_unknown_keys() {
        let directory = tempfile::tempdir().unwrap();
        let signer = signing_key(7);
        let releases = assets(directory.path(), "1.0.0", &signer);
        let bytes = feed::signed(releases, &signer).unwrap();
        assert_eq!(
            feed::decode(&bytes, REPOSITORY, Channel::Stable, &ring(&signer))
                .unwrap()
                .len(),
            2
        );
        assert!(feed::decode(&bytes, REPOSITORY, Channel::Stable, &ring(&signing_key(8))).is_err());
        let changed = String::from_utf8(bytes)
            .unwrap()
            .replace("Sailry.app", "Sailry.bad");
        assert!(
            feed::decode(
                changed.as_bytes(),
                REPOSITORY,
                Channel::Stable,
                &ring(&signer)
            )
            .is_err()
        );
    }
}

mod channel_merge {
    use super::*;

    fn merge(
        current: &[u8],
        assets: &Path,
        channel: Channel,
        version: &str,
        signer: &SigningKey,
    ) -> Result<Vec<u8>> {
        feed::merge(
            Some(current),
            assets,
            channel,
            REPOSITORY,
            &format!("v{version}"),
            &ring(signer),
            signer,
        )
    }

    #[test]
    fn creates_exact_version_catalog_without_history() {
        let signer = signing_key(7);
        let directory = tempfile::tempdir().unwrap();
        assets(directory.path(), "1.0.0-alpha.2", &signer);
        let bytes = feed::merge(
            None,
            directory.path(),
            Channel::Experience,
            REPOSITORY,
            "v1.0.0-alpha.2",
            &ring(&signer),
            &signer,
        )
        .unwrap();
        let releases =
            feed::decode(&bytes, REPOSITORY, Channel::Experience, &ring(&signer)).unwrap();
        assert_eq!(releases.len(), 2);
        assert!(
            releases
                .iter()
                .all(|release| release.version == "1.0.0-alpha.2"
                    && release.url.contains("/releases/download/v1.0.0-alpha.2/"))
        );
        assert!(
            feed::merge(
                Some(b"invalid previous feed"),
                directory.path(),
                Channel::Experience,
                REPOSITORY,
                "v1.0.0-alpha.2",
                &ring(&signer),
                &signer
            )
            .is_err()
        );
    }

    #[test]
    fn stable_refuses_prereleases() {
        let current = feed::signed(Vec::new(), &signing_key(7)).unwrap();
        let directory = tempfile::tempdir().unwrap();
        assets(directory.path(), "1.0.0-alpha.1", &signing_key(7));
        assert!(
            merge(
                &current,
                directory.path(),
                Channel::Stable,
                "1.0.0-alpha.1",
                &signing_key(7)
            )
            .is_err()
        );
    }

    #[test]
    fn experience_keeps_previews_and_graduates() {
        let signer = signing_key(7);
        let current = feed::signed(Vec::new(), &signer).unwrap();
        let preview = tempfile::tempdir().unwrap();
        assets(preview.path(), "1.0.0-alpha.1", &signer);
        let current = merge(
            &current,
            preview.path(),
            Channel::Experience,
            "1.0.0-alpha.1",
            &signer,
        )
        .unwrap();
        let stable = tempfile::tempdir().unwrap();
        assets(stable.path(), "1.0.0", &signer);
        let current = merge(
            &current,
            stable.path(),
            Channel::Experience,
            "1.0.0",
            &signer,
        )
        .unwrap();
        let releases =
            feed::decode(&current, REPOSITORY, Channel::Experience, &ring(&signer)).unwrap();
        assert_eq!(releases.len(), 4);
        assert_eq!(releases[0].version, "1.0.0-alpha.1");
        assert_eq!(releases[3].version, "1.0.0");
        assert_eq!(releases[0].name, "Sailry-aarch64-apple-darwin.zip");
        assert_eq!(releases[0].name, releases[2].name);
        assert_eq!(
            releases[0].url,
            format!(
                "https://github.com/{REPOSITORY}/releases/download/v1.0.0-alpha.1/{}",
                releases[0].name
            )
        );
        assert_eq!(
            releases[2].url,
            format!(
                "https://github.com/{REPOSITORY}/releases/download/v1.0.0/{}",
                releases[2].name
            )
        );
    }

    #[test]
    fn rejects_changed_published_identity() {
        let signer = signing_key(7);
        let first = tempfile::tempdir().unwrap();
        let releases = assets(first.path(), "1.0.0", &signer);
        let current = feed::signed(releases, &signer).unwrap();
        let second = tempfile::tempdir().unwrap();
        assets(second.path(), "1.0.0", &signer);
        let target = "aarch64-apple-darwin";
        let name = format!("Sailry-{target}.zip");
        let source = second.path().join("changed.unsigned.zip");
        let archive_path = second.path().join(&name);
        fs::remove_file(&archive_path).unwrap();
        unsigned(&source, b"different updater fixture");
        archive::sign(&source, &archive_path, &name, &signer, &ring(&signer)).unwrap();
        fs::remove_file(source).unwrap();
        let changed = feed::describe(
            &archive_path,
            "1.0.0",
            target,
            "13.0",
            REPOSITORY,
            "v1.0.0",
            &ring(&signer),
        )
        .unwrap();
        fs::write(
            second.path().join(format!("{}.update.json", changed.name)),
            serde_json::to_vec(&changed).unwrap(),
        )
        .unwrap();
        assert!(merge(&current, second.path(), Channel::Stable, "1.0.0", &signer).is_err());
    }
}
