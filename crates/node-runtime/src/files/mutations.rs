//! Revision-checked file actions reuse the ordinary rename and native Trash kernels.
use super::{remove_verified, rename, version};
use sailry_protocol::Fault;
use std::path::Path;

pub(crate) fn delete(root: &Path, path: &str, expected: &str) -> Result<(), Fault> {
    let stamp = version::current(root, path)?;
    remove_verified(root, path, expected, &stamp)
}

pub(crate) fn move_file(root: &Path, from: &str, to: &str, expected: &str) -> Result<(), Fault> {
    if blake3::Hash::from_hex(expected).is_err() {
        return Err(Fault::invalid("invalid file source revision"));
    }
    let stamp = version::current(root, from)?;
    version::verify(root, from, expected, &stamp)?;
    rename(root, from, to)
}

#[cfg(test)]
mod tests;
