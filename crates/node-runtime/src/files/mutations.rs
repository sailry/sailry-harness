//! Revision-checked file actions reuse the ordinary rename and native Trash kernels.
use super::{remove_verified, rename, version};
use sailry_protocol::Fault;

pub(crate) fn delete<'a>(
    root: impl Into<super::Scope<'a>>,
    path: &str,
    expected: &str,
) -> Result<(), Fault> {
    let root = root.into();
    let stamp = version::current(root, path)?;
    remove_verified(root, path, expected, &stamp)
}

pub(crate) fn move_file<'a>(
    root: impl Into<super::Scope<'a>>,
    from: &str,
    to: &str,
    expected: &str,
) -> Result<(), Fault> {
    let root = root.into();
    if blake3::Hash::from_hex(expected).is_err() {
        return Err(Fault::invalid("invalid file source revision"));
    }
    let stamp = version::current(root, from)?;
    version::verify(root, from, expected, &stamp)?;
    rename(root, from, to)
}

#[cfg(test)]
mod tests;
