//! Offline release preparation: use an existing publisher seed, never generate keys or publish.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::Signer as _;
use serde::{Deserialize, Serialize};
use sha2::Digest as _;
use std::{
    fs,
    io::{Read as _, Seek as _, Write as _},
    path::{Path, PathBuf},
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    releases: Vec<Release>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Release {
    version: String,
    target: String,
    minimum_system: String,
    name: String,
    url: String,
    sha256: String,
    size: u64,
    bundle: String,
    executable: String,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("update release preparation failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    prepare(&args)
}

fn prepare(args: &[std::ffi::OsString]) -> Result<(), Box<dyn std::error::Error>> {
    if args.len() != 4 {
        return Err("usage: cargo run -p sailry-desktop --example sign_update -- PUBLISHER_SEED PAYLOAD_JSON INPUT_DIRECTORY NEW_OUTPUT_DIRECTORY".into());
    }
    let key_path = PathBuf::from(&args[0]);
    let seed: [u8; 32] = fs::read(key_path)?
        .try_into()
        .map_err(|_| "publisher seed must contain exactly 32 raw bytes")?;
    let key = ed25519_dalek::SigningKey::from_bytes(&seed);
    let archive_key = zipsign_api::SigningKey::from_bytes(&seed);
    let payload = fs::read(&args[1])?;
    if payload.len() > 1024 * 1024 {
        return Err("release payload exceeds its size limit".into());
    }
    let mut payload: Payload = serde_json::from_slice(&payload)?;
    let input = PathBuf::from(&args[2]).canonicalize()?;
    let output = PathBuf::from(&args[3]);
    if output.exists() {
        return Err("the output directory must not already exist".into());
    }
    for release in &payload.releases {
        semver::Version::parse(&release.version)?;
        let layout = match release.target.as_str() {
            "aarch64-apple-darwin" | "x86_64-apple-darwin" => {
                ("Sailry.app", "Contents/MacOS/sailry-desktop")
            }
            "aarch64-pc-windows-msvc" | "x86_64-pc-windows-msvc" => {
                ("Sailry", "sailry-desktop.exe")
            }
            _ => return Err("unsupported release target".into()),
        };
        let name = Path::new(&release.name);
        if name.components().count() != 1
            || name.file_name().and_then(|name| name.to_str()) != Some(release.name.as_str())
            || release.name.contains(['\\', ':'])
            || !release.name.ends_with(".zip")
            || (release.bundle.as_str(), release.executable.as_str()) != layout
        {
            return Err("invalid release package path or layout".into());
        }
        let url = url::Url::parse(&release.url)?;
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err("release URLs require HTTPS without credentials or fragments".into());
        }
        let file = input.join(&release.name).canonicalize()?;
        if !file.starts_with(&input) || !file.is_file() {
            return Err("release input points outside its input directory".into());
        }
    }
    fs::create_dir(&output)?;
    for release in &mut payload.releases {
        let mut source = fs::File::open(input.join(&release.name))?;
        let path = output.join(&release.name);
        let mut signed = fs::OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)?;
        zipsign_api::sign::copy_and_sign_zip(
            &mut source,
            &mut signed,
            std::slice::from_ref(&archive_key),
            Some(release.name.as_bytes()),
        )?;
        signed.sync_all()?;
        signed.rewind()?;
        let mut digest = sha2::Sha256::new();
        let mut buffer = [0; 64 * 1024];
        loop {
            let length = signed.read(&mut buffer)?;
            if length == 0 {
                break;
            }
            digest.update(&buffer[..length]);
        }
        release.sha256 = format!("{:x}", digest.finalize());
        release.size = signed.metadata()?.len();
    }
    let payload = serde_json::to_string(&payload)?;
    let signature = STANDARD.encode(key.sign(payload.as_bytes()).to_bytes());
    let manifest =
        format!("{{\"version\":1,\"payload\":{payload},\"signature\":\"{signature}\"}}\n");
    let mut destination = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output.join("update-v1.json"))?;
    destination.write_all(manifest.as_bytes())?;
    destination.sync_all()?;
    println!("Signed update artifacts prepared; nothing was published");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Envelope {
        version: u32,
        payload: Box<serde_json::value::RawValue>,
        signature: String,
    }

    #[test]
    fn signs_metadata_without_overwriting_inputs() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input");
        fs::create_dir(&input).unwrap();
        let name = "Sailry-9.9.9-aarch64-apple-darwin.zip";
        let mut archive = zip::ZipWriter::new(fs::File::create(input.join(name)).unwrap());
        archive
            .start_file(
                "Sailry.app/isolated-fixture",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive.write_all(b"not an executable package").unwrap();
        archive.finish().unwrap();
        let original = fs::read(input.join(name)).unwrap();
        // This deterministic seed belongs only to the isolated signing fixture.
        let seed = [7; 32];
        let key = directory.path().join("fixture-seed");
        fs::write(&key, seed).unwrap();
        let payload = directory.path().join("payload.json");
        fs::write(
            &payload,
            serde_json::to_vec(&serde_json::json!({"releases":[{
                "version":"9.9.9", "target":"aarch64-apple-darwin", "minimum_system":"13.0",
                "name":name, "url":"https://example.test/package.zip", "sha256":"", "size":0,
                "bundle":"Sailry.app", "executable":"Contents/MacOS/sailry-desktop"
            }]}))
            .unwrap(),
        )
        .unwrap();
        let output = directory.path().join("signed");
        let args = [&key, &payload, &input, &output].map(|path| path.as_os_str().to_owned());
        prepare(&args).unwrap();
        let public = ed25519_dalek::SigningKey::from_bytes(&seed).verifying_key();
        self_update::verify_signature(&output.join(name), &[public.to_bytes()]).unwrap();
        let manifest = fs::read(output.join("update-v1.json")).unwrap();
        let envelope: Envelope = serde_json::from_slice(&manifest).unwrap();
        assert_eq!(envelope.version, 1);
        let signature =
            ed25519_dalek::Signature::from_slice(&STANDARD.decode(envelope.signature).unwrap())
                .unwrap();
        public
            .verify_strict(envelope.payload.get().as_bytes(), &signature)
            .unwrap();
        let releases: Payload = serde_json::from_str(envelope.payload.get()).unwrap();
        let signed = fs::read(output.join(name)).unwrap();
        assert_eq!(releases.releases[0].size, signed.len() as u64);
        assert_eq!(
            releases.releases[0].sha256,
            format!("{:x}", sha2::Sha256::digest(&signed))
        );
        assert_eq!(fs::read(input.join(name)).unwrap(), original);
        assert!(prepare(&args).is_err());
        assert_eq!(fs::read(output.join("update-v1.json")).unwrap(), manifest);
    }
}
