use super::*;
use cbc::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use sha2::Digest;

pub(super) fn key(password: &[u8]) -> Zeroizing<[u8; 16]> {
    let mut key = Zeroizing::new([0; 16]);
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password, b"saltysalt", 1003, &mut *key);
    key
}

pub(super) fn decrypt(key: &[u8; 16], domain: &str, encrypted: &[u8]) -> Result<Zeroizing<String>> {
    let encrypted = encrypted
        .strip_prefix(b"v10")
        .ok_or("browser_chrome_cipher")?;
    let mut bytes = Zeroizing::new(encrypted.to_vec());
    let plain = cbc::Decryptor::<aes::Aes128>::new(key.into(), &[b' '; 16].into())
        .decrypt_padded_mut::<Pkcs7>(&mut bytes)
        .map_err(|_| "browser_chrome_cipher")?;
    let hash = sha2::Sha256::digest(domain.as_bytes());
    let value = plain
        .strip_prefix(hash.as_slice())
        .ok_or("browser_chrome_cipher")?;
    Ok(Zeroizing::new(
        std::str::from_utf8(value)
            .map_err(|_| "browser_chrome_cipher")?
            .into(),
    ))
}
