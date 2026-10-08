//! Read-only package checks use the same strict loader as theme import.
use std::path::Path;

use super::package::Loaded;

pub(crate) fn run(path: &Path) -> Result<String, String> {
    let loaded = Loaded::read(path, true)?;
    Ok(format!(
        "Theme {} passed: light and dark variants, {} unique images",
        loaded.manifest.id,
        loaded.images.len()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_directory_and_archive() {
        let root = tempfile::tempdir().unwrap();
        let directory = crate::theme::fixture::write(root.path(), "fixture");
        let expected = "Theme fixture passed: light and dark variants, 2 unique images";
        assert_eq!(run(&directory).unwrap(), expected);
        let archive = root.path().join("fixture.zip");
        Loaded::read(&directory, true)
            .unwrap()
            .export(&archive)
            .unwrap();
        assert_eq!(run(&archive).unwrap(), expected);
    }

    #[test]
    fn rejects_missing_resources() {
        let root = tempfile::tempdir().unwrap();
        let directory = crate::theme::fixture::write(root.path(), "fixture");
        std::fs::remove_dir_all(directory.join("assets")).unwrap();
        assert!(run(&directory).unwrap_err().contains("assets/"));
    }
}
