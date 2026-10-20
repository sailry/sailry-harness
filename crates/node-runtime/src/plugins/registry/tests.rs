use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use std::io::{Cursor, Write};

fn records(payload: String) -> (verification::Envelope, verification::Keys) {
    let key = SigningKey::from_bytes(&[25; 32]);
    let signature = key.sign(payload.as_bytes());
    let hash = serde_json::from_str::<serde_json::Value>(&payload).unwrap()["content_hash"].clone();
    (
        serde_json::from_value(serde_json::json!({"signed":true,"key_id":"fixture","payload":payload,"signature":STANDARD.encode(signature.to_bytes()),"content_hash":hash,"hash_version":1})).unwrap(),
        serde_json::from_value(serde_json::json!({"keys":[{"key_id":"fixture","algorithm":"Ed25519","public_key_raw_b64":STANDARD.encode(key.verifying_key().to_bytes()),"status":"active","issuer":"skillhub.cn"}]})).unwrap(),
    )
}

fn payload(hash: &str, bytes: &[u8], count: usize) -> String {
    serde_json::json!({"v":1,"issuer":"skillhub.cn","skill_slug":"analysis","skill_version":"1.0.0","version_id":1,"publisher_id":42,"content_hash":hash,"package_md5":format!("{:x}",md5::Md5::digest(bytes)),"file_count":count}).to_string()
}

fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes) in entries {
        zip.start_file(*path, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn resolved() -> RegistryResolved {
    RegistryResolved {
        namespace: "example".into(),
        slug: "analysis".into(),
        version: "1.0.0".into(),
        version_id: "1".into(),
        content_hash: String::new(),
        package_md5: String::new(),
        archive_sha256: String::new(),
    }
}

#[test]
fn authenticates_original_payload_bytes() {
    let bytes = archive(&[("SKILL.md", b"body")]);
    let body = payload(&"a".repeat(64), &bytes, 1);
    let (signature, keys) = records(format!("  {body}\n"));
    assert!(verification::signature(&signature, &keys, "analysis", "1.0.0").is_ok());
    assert!(verification::signature(&signature, &keys, "other", "1.0.0").is_err());
    assert!(verification::signature(&signature, &keys, "analysis", "1.0.1").is_err());
    let mut value = serde_json::json!({"signed":true,"key_id":"fixture","payload":body,"signature":STANDARD.encode([0;64]),"content_hash":"a".repeat(64),"hash_version":1});
    let invalid: verification::Envelope = serde_json::from_value(value.clone()).unwrap();
    assert!(verification::signature(&invalid, &keys, "analysis", "1.0.0").is_err());
    value["signed"] = false.into();
    let unsigned: verification::Envelope = serde_json::from_value(value).unwrap();
    assert!(verification::signature(&unsigned, &keys, "analysis", "1.0.0").is_err());
    let (signature, _) = records(body);
    for status in ["revoked", "retired"] {
        let key = SigningKey::from_bytes(&[25; 32]);
        let revoked: verification::Keys = serde_json::from_value(serde_json::json!({"keys":[{"key_id":"fixture","algorithm":"Ed25519","public_key_raw_b64":STANDARD.encode(key.verifying_key().to_bytes()),"status":status,"issuer":"skillhub.cn"}]})).unwrap();
        assert!(verification::signature(&signature, &revoked, "analysis", "1.0.0").is_err());
    }
}

#[test]
fn confines_signed_archive_entries() {
    let hash = format!(
        "{:x}",
        Sha256::digest(format!("SKILL.md:{:x}\n", Sha256::digest(b"body")).as_bytes())
    );
    for path in [
        "../SKILL.md",
        "/SKILL.md",
        "a/../SKILL.md",
        "a//SKILL.md",
        "a\\SKILL.md",
    ] {
        let bytes = archive(&[(path, b"body")]);
        let payload: verification::Payload =
            serde_json::from_str(&payload(&hash, &bytes, 1)).unwrap();
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(&bytes).unwrap();
        assert!(
            verification::archive(file, resolved(), &payload, &CancellationToken::new()).is_err(),
            "{path}"
        );
    }
    let bytes = archive(&[("SKILL.md", b"body"), ("_meta.json", b"unsigned metadata")]);
    let payload: verification::Payload = serde_json::from_str(&payload(&hash, &bytes, 1)).unwrap();
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(&bytes).unwrap();
    let (_, source) =
        verification::archive(file, resolved(), &payload, &CancellationToken::new()).unwrap();
    assert_eq!(
        source.archive_sha256,
        format!("{:x}", Sha256::digest(&bytes))
    );
    let mut changed = bytes;
    changed[0] ^= 1;
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(&changed).unwrap();
    assert!(verification::archive(file, resolved(), &payload, &CancellationToken::new()).is_err());
}

#[test]
fn accepts_authenticated_metadata_hashes() {
    let bytes = archive(&[
        ("SKILL.md", b"body"),
        ("_meta.json", b"metadata"),
        ("thumbs.DB", b"thumbnail"),
    ]);
    let tree = format!(
        "SKILL.md:{:x}\n_meta.json:{:x}\n",
        Sha256::digest(b"body"),
        Sha256::digest(b"metadata")
    );
    let hash = format!("{:x}", Sha256::digest(tree.as_bytes()));
    let claims: verification::Payload = serde_json::from_str(&payload(&hash, &bytes, 2)).unwrap();
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(&bytes).unwrap();
    assert!(verification::archive(file, resolved(), &claims, &CancellationToken::new()).is_ok());
}

#[test]
fn validates_coordinates_without_git_revisions() {
    for slug in ["", "..", "a/b", "a?key=secret"] {
        assert!(
            validate(&RegistrySource {
                namespace: None,
                slug: slug.into(),
                version: None
            })
            .is_err()
        );
    }
    let first = resolved();
    let mut second = first.clone();
    second.version = "2.0.0".into();
    assert_eq!(name(&first), name(&second));
    second.namespace = "another".into();
    assert_ne!(name(&first), name(&second));
}

#[test]
fn projects_verified_legacy_metadata() {
    let source = resolved();
    let raw = "---\nname: Analysis / Review\nslug: analysis\ndescription: Inspect a sample\nmetadata:\n  clawdbot: {requires: {bins: [python3]}}\n---\nBody\n";
    assert!(super::super::skills::parse_document(raw).is_err());
    let projected = document::normalize(raw.as_bytes(), &source).unwrap();
    let parsed = super::super::skills::parse("analysis", &projected).unwrap();
    assert_eq!(parsed.name, "analysis");
    assert_eq!(parsed.display_name.as_deref(), Some("Analysis / Review"));
    assert_eq!(super::super::skills::body(&projected), "Body\n");
    let header = super::super::skills::header(&projected).unwrap();
    assert_eq!(
        header["metadata"]["clawdbot"],
        "{\"requires\":{\"bins\":[\"python3\"]}}"
    );
    for fields in [
        "name: other",
        "name: Analysis / Review\nslug: other",
        "name: analysis\nmetadata: []",
        "name: analysis\nmetadata: {display-name: {invalid: value}}",
        "name: analysis\nname: analysis",
    ] {
        let raw = format!("---\n{fields}\ndescription: Inspect a sample\n---\nBody");
        assert!(
            document::normalize(raw.as_bytes(), &source).is_err(),
            "{fields}"
        );
    }
}
