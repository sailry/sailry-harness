//! The library handles archive extraction; this adapter verifies Sailry's package contract.
use super::{Failure, Result, manifest::Release};
use serde::Deserialize;
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const MAX_EXTRACTED: u64 = 8 * 1024 * 1024 * 1024;

pub(super) fn verify_archive(
    path: &Path,
    release: &Release,
    keys: &[self_update::VerifyingKey],
) -> Result<()> {
    if keys.is_empty() {
        return Err(Failure::new(
            "updates_source_unready",
            "archive verification requires trusted publisher keys",
        ));
    }
    if path.file_name().and_then(|name| name.to_str()) != Some(&release.name)
        || fs::metadata(path).map_err(Failure::io)?.len() != release.size
    {
        return Err(Failure::new(
            "updates_package_invalid",
            "the staged archive name or size differs from signed metadata",
        ));
    }
    // The library's standalone checksum verifier is private. Hash staging here;
    // the macOS library installer repeats its own checksum gate before swapping.
    use sha2::Digest as _;
    let mut digest = sha2::Sha256::new();
    let mut input = fs::File::open(path)
        .map_err(Failure::io)?
        .take(release.size + 1);
    let copied = std::io::copy(&mut input, &mut digest).map_err(Failure::io)?;
    if copied != release.size || format!("{:x}", digest.finalize()) != release.sha256 {
        return Err(Failure::new(
            "updates_digest_invalid",
            "the update archive checksum does not match",
        ));
    }
    self_update::verify_signature(path, keys).map_err(|_| {
        Failure::new(
            "updates_signature_invalid",
            "the update archive was not signed by a trusted publisher",
        )
    })?;
    let mut archive =
        zip::ZipArchive::new(fs::File::open(path).map_err(Failure::io)?).map_err(|_| {
            Failure::new(
                "updates_package_invalid",
                "the update archive is not a ZIP package",
            )
        })?;
    let mut size = 0_u64;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|_| Failure::new("updates_package_invalid", "the ZIP directory is invalid"))?;
        if entry.enclosed_name().is_none() {
            return Err(Failure::new(
                "updates_package_invalid",
                "the ZIP contains an unsafe path",
            ));
        }
        size = size
            .checked_add(entry.size())
            .filter(|size| *size <= MAX_EXTRACTED)
            .ok_or_else(|| {
                Failure::new(
                    "updates_package_invalid",
                    "the uncompressed update exceeds its size limit",
                )
            })?;
    }
    Ok(())
}

pub(super) fn extract(archive: &Path, release: &Release, destination: &Path) -> Result<PathBuf> {
    self_update::Extract::from_source(archive)
        .extract_into(destination)
        .map_err(|_| {
            Failure::new(
                "updates_package_invalid",
                "the update package could not be extracted",
            )
        })?;
    let root = destination.join(&release.bundle);
    validate(&root, release)?;
    Ok(root)
}

#[derive(Deserialize)]
struct Build {
    version: u32,
    target: String,
    application_version: String,
}

#[derive(Deserialize)]
struct Runtime {
    version: u32,
    target: String,
    executable: String,
}

pub(super) fn validate(root: &Path, release: &Release) -> Result<()> {
    if !root.is_dir() {
        return Err(Failure::new(
            "updates_package_invalid",
            "the update has no application bundle",
        ));
    }
    let executable = confined(root, &root.join(&release.executable))?;
    verify_executable(&executable, &release.target)?;
    let macos = release.target.ends_with("apple-darwin");
    let resources = if macos {
        root.join("Contents/Resources")
    } else {
        root.to_path_buf()
    };
    let build: Build = serde_json::from_slice(&read(
        &confined(root, &resources.join("build.json"))?,
        1024 * 1024,
    )?)
    .map_err(|_| {
        Failure::new(
            "updates_package_invalid",
            "the packaged build metadata is invalid",
        )
    })?;
    if build.version != 1
        || build.target != release.target
        || build.application_version != release.version
    {
        return Err(Failure::new(
            "updates_package_invalid",
            "the packaged build metadata differs from signed update metadata",
        ));
    }
    let runtime_root = resources.join("office-runtime");
    let runtime: Runtime = serde_json::from_slice(&read(
        &confined(root, &runtime_root.join("runtime.json"))?,
        64 * 1024,
    )?)
    .map_err(|_| {
        Failure::new(
            "updates_package_invalid",
            "the packaged Office runtime metadata is invalid",
        )
    })?;
    let expected_python = if macos {
        "python/bin/python3.12"
    } else {
        "python/python.exe"
    };
    if runtime.version != 1
        || runtime.target != release.target
        || runtime.executable != expected_python
    {
        return Err(Failure::new(
            "updates_package_invalid",
            "the packaged Office runtime differs from this update target",
        ));
    }
    let python = confined(root, &runtime_root.join(&runtime.executable))?;
    verify_executable(&python, &release.target)?;
    if macos {
        validate_plist(root, release)?;
        #[cfg(target_os = "macos")]
        {
            let result = std::process::Command::new("/usr/bin/codesign")
                .args(["--verify", "--deep", "--strict"])
                .arg(root)
                .output()
                .map_err(|_| {
                    Failure::new(
                        "updates_signature_invalid",
                        "the macOS code signature could not be verified",
                    )
                })?;
            if !result.status.success() {
                return Err(Failure::new(
                    "updates_signature_invalid",
                    "the packaged macOS code signature is invalid",
                ));
            }
        }
    }
    Ok(())
}

fn validate_plist(root: &Path, release: &Release) -> Result<()> {
    let bytes = read(
        &confined(root, &root.join("Contents/Info.plist"))?,
        128 * 1024,
    )?;
    let xml = std::str::from_utf8(&bytes).map_err(|_| {
        Failure::new(
            "updates_package_invalid",
            "the package requires an XML application plist",
        )
    })?;
    // The packaged XML plist carries Apple's standard DOCTYPE. roxmltree never
    // fetches external DTDs; allowing that declaration preserves plist parsing.
    let document = roxmltree::Document::parse_with_options(
        xml,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        },
    )
    .map_err(|_| {
        Failure::new(
            "updates_package_invalid",
            "the application plist is invalid",
        )
    })?;
    let dict = document
        .root_element()
        .children()
        .find(|node| node.has_tag_name("dict"))
        .ok_or_else(|| {
            Failure::new(
                "updates_package_invalid",
                "the application plist has no dictionary",
            )
        })?;
    let value = |key: &str| {
        dict.children()
            .filter(|node| node.has_tag_name("key"))
            .find(|node| node.text() == Some(key))
            .and_then(|node| node.next_sibling_element())
            .filter(|node| node.has_tag_name("string"))
            .and_then(|node| node.text())
    };
    if value("CFBundleIdentifier") != Some("ai.sailry.desktop")
        || value("CFBundleExecutable") != Some("sailry-desktop")
        || value("CFBundleShortVersionString") != Some(release.version.as_str())
        || value("LSMinimumSystemVersion") != Some(release.minimum_system.as_str())
    {
        return Err(Failure::new(
            "updates_package_invalid",
            "the application plist differs from signed update metadata",
        ));
    }
    Ok(())
}

fn confined(root: &Path, path: &Path) -> Result<PathBuf> {
    let root = root.canonicalize().map_err(Failure::io)?;
    let path = path.canonicalize().map_err(|_| {
        Failure::new(
            "updates_package_invalid",
            "a required bundled resource is missing",
        )
    })?;
    if !path.starts_with(root) {
        return Err(Failure::new(
            "updates_package_invalid",
            "a bundled resource points outside the application",
        ));
    }
    Ok(path)
}

fn read(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = fs::File::open(path).map_err(Failure::io)?;
    if file.metadata().map_err(Failure::io)?.len() > limit {
        return Err(Failure::new(
            "updates_package_invalid",
            "bundled metadata exceeds its size limit",
        ));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(Failure::io)?;
    if bytes.len() as u64 > limit {
        return Err(Failure::new(
            "updates_package_invalid",
            "bundled metadata exceeds its size limit",
        ));
    }
    Ok(bytes)
}

fn verify_executable(path: &Path, target: &str) -> Result<()> {
    let mut file = fs::File::open(path).map_err(Failure::io)?;
    let mut header = [0_u8; 64];
    file.read_exact(&mut header).map_err(|_| {
        Failure::new(
            "updates_package_invalid",
            "the packaged executable is incomplete",
        )
    })?;
    // self_update does not inspect executable architecture. Sailry packages use a thin Mach-O
    // or PE image; verify its native header before accepting the publisher's target label.
    let expected_arm = target.starts_with("aarch64-");
    let valid = if target.ends_with("apple-darwin") {
        u32::from_le_bytes(header[..4].try_into().unwrap()) == 0xfeed_facf
            && u32::from_le_bytes(header[4..8].try_into().unwrap())
                == if expected_arm {
                    0x0100_000c
                } else {
                    0x0100_0007
                }
    } else {
        if &header[..2] != b"MZ" {
            return Err(Failure::new(
                "updates_package_invalid",
                "the packaged executable is not a PE image",
            ));
        }
        let offset = u32::from_le_bytes(header[60..64].try_into().unwrap());
        file.seek(SeekFrom::Start(offset.into()))
            .map_err(Failure::io)?;
        let mut pe = [0_u8; 6];
        file.read_exact(&mut pe)
            .map_err(|_| Failure::new("updates_package_invalid", "the PE header is incomplete"))?;
        &pe[..4] == b"PE\0\0"
            && u16::from_le_bytes(pe[4..6].try_into().unwrap())
                == if expected_arm { 0xaa64 } else { 0x8664 }
    };
    if !valid {
        return Err(Failure::new(
            "updates_platform",
            "the packaged executable architecture differs from signed metadata",
        ));
    }
    Ok(())
}
