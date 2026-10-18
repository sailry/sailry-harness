//! Market provenance is published with the same verified package snapshot.
use super::{Loaded, Manifest};
use sha2::{Digest as _, Sha256};

pub(in crate::theme) const MANIFEST_BYTES: usize = super::resources::MANIFEST_LIMIT;
pub(in crate::theme) const FILE_BYTES: usize = super::resources::FILE_LIMIT;
pub(in crate::theme) const PACKAGE_BYTES: usize = super::resources::PACKAGE_LIMIT;
pub(in crate::theme) const SOURCE_FILE: &str = ".sailry-source.json";

pub(in crate::theme) fn resource_path(path: &str) -> Result<(), String> {
    super::resources::validate_path(path)
}

pub(in crate::theme) fn manifest(bytes: &[u8]) -> Result<Manifest, String> {
    super::resources::manifest(bytes)
}

impl Loaded {
    pub(in crate::theme) fn digest(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(b"Sailry theme package v1\0");
        for (path, bytes) in self
            .files
            .iter()
            .filter(|(path, _)| path.as_str() != SOURCE_FILE)
        {
            hash.update((path.len() as u64).to_be_bytes());
            hash.update(path.as_bytes());
            hash.update((bytes.len() as u64).to_be_bytes());
            hash.update(bytes);
        }
        format!("sha256:{:x}", hash.finalize())
    }

    pub(in crate::theme) fn market_source(&mut self, bytes: Option<Vec<u8>>) {
        if let Some(bytes) = bytes {
            self.files.insert(SOURCE_FILE.into(), bytes);
        } else {
            self.files.remove(SOURCE_FILE);
        }
    }
}
