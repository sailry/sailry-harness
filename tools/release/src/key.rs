use crate::{Result, invalid};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::SigningKey;
use std::{collections::BTreeSet, env, fs, path::Path};
use zeroize::{Zeroize, Zeroizing};

const SIGNING_KEY: &str = "SAILRY_UPDATE_SIGNING_KEY";
const MAX_RING: u64 = 16 * 1024;

pub(crate) fn read_ring(path: &Path) -> Result<Vec<[u8; 32]>> {
    if fs::metadata(path)?.len() > MAX_RING {
        return Err(invalid("update public-key ring exceeds its limit").into());
    }
    let contents = fs::read_to_string(path)?;
    let mut unique = BTreeSet::new();
    let mut keys = Vec::new();
    for line in contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let bytes = STANDARD
            .decode(line)
            .map_err(|_| invalid("update public key is not valid base64"))?;
        let bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| invalid("update public keys must have 32 bytes"))?;
        let key = ed25519_dalek::VerifyingKey::from_bytes(&bytes)
            .map_err(|_| invalid("update public key is invalid"))?;
        if key.is_weak() {
            return Err(invalid("update public key is weak").into());
        }
        if !unique.insert(bytes) {
            return Err(invalid("update public-key ring contains duplicates").into());
        }
        zipsign_api::VerifyingKey::from_bytes(&bytes)
            .map_err(|_| invalid("update public key is invalid for archive verification"))?;
        keys.push(bytes);
    }
    if keys.is_empty() {
        return Err(invalid("update public-key ring is empty").into());
    }
    Ok(keys)
}

pub(crate) fn read_signer(ring: &[[u8; 32]]) -> Result<SigningKey> {
    let encoded = Zeroizing::new(
        env::var(SIGNING_KEY).map_err(|_| invalid("update signing key is not configured"))?,
    );
    decode_signer(encoded.trim(), ring)
}

fn decode_signer(encoded: &str, ring: &[[u8; 32]]) -> Result<SigningKey> {
    let mut decoded = Zeroizing::new(
        STANDARD
            .decode(encoded)
            .map_err(|_| invalid("update signing key is not valid base64"))?,
    );
    let mut seed: [u8; 32] = decoded
        .as_slice()
        .try_into()
        .map_err(|_| invalid("update signing key must have 32 bytes"))?;
    decoded.zeroize();
    let signer = SigningKey::from_bytes(&seed);
    seed.zeroize();
    if !ring
        .iter()
        .any(|key| key == signer.verifying_key().as_bytes())
    {
        return Err(invalid("update signing key is outside the public-key ring").into());
    }
    Ok(signer)
}

#[cfg(test)]
pub(crate) fn signer(encoded: &str, ring: &[[u8; 32]]) -> Result<SigningKey> {
    decode_signer(encoded, ring)
}
