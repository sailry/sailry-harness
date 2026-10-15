use crate::{Result, invalid};
use ed25519_dalek::SigningKey;
use std::{
    fs::{File, OpenOptions},
    io::{Seek, SeekFrom},
    path::{Component, Path},
};

pub(crate) fn sign(
    input: &Path,
    output: &Path,
    name: &str,
    signer: &SigningKey,
    ring: &[[u8; 32]],
) -> Result<()> {
    validate_name(name)?;
    if input == output || output.file_name().and_then(|value| value.to_str()) != Some(name) {
        return Err(invalid("signed archive output must use its authenticated name").into());
    }
    let mut input = File::open(input)?;
    let mut output = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(output)?;
    let mut seed = signer.to_bytes();
    let archive_signer = zipsign_api::SigningKey::from_bytes(&seed);
    zeroize::Zeroize::zeroize(&mut seed);
    zipsign_api::sign::copy_and_sign_zip(
        &mut input,
        &mut output,
        std::slice::from_ref(&archive_signer),
        Some(name.as_bytes()),
    )?;
    output.sync_all()?;
    output.seek(SeekFrom::Start(0))?;
    zipsign_api::verify::verify_zip(&mut output, &keys(ring)?, Some(name.as_bytes()))?;
    Ok(())
}

pub(crate) fn verify(path: &Path, name: &str, ring: &[[u8; 32]]) -> Result<()> {
    validate_name(name)?;
    if path.file_name().and_then(|value| value.to_str()) != Some(name) {
        return Err(invalid("update archive name does not match its metadata").into());
    }
    let mut archive = File::open(path)?;
    zipsign_api::verify::verify_zip(&mut archive, &keys(ring)?, Some(name.as_bytes()))?;
    Ok(())
}

fn keys(ring: &[[u8; 32]]) -> Result<Vec<zipsign_api::VerifyingKey>> {
    ring.iter()
        .map(|bytes| {
            zipsign_api::VerifyingKey::from_bytes(bytes).map_err(|_| {
                invalid("update public key is invalid for archive verification").into()
            })
        })
        .collect()
}

fn validate_name(name: &str) -> Result<()> {
    let path = Path::new(name);
    if name.is_empty()
        || name.contains(['\\', ':'])
        || !name.ends_with(".zip")
        || path.components().count() != 1
        || !path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err(invalid("update archive name is unsafe").into());
    }
    Ok(())
}
