use objc2_foundation::{
    NSFileManager, NSSearchPathDirectory, NSSearchPathDomainMask, NSString, NSURL,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Recover only this exact UUID leaf; never enumerate or purge the user's Trash.
pub(crate) struct Entry {
    pub name: String,
    pub source: PathBuf,
    pub recycled: PathBuf,
    recovery: tempfile::TempDir,
}

impl Entry {
    pub fn new(root: &Path, name: String) -> Self {
        assert_eq!(Path::new(&name).components().count(), 1);
        // Trash belongs to the source volume, which may differ from HOME.
        // Resolve that directory without enumerating any recycled user items.
        let source = NSURL::fileURLWithPath(&NSString::from_str(root.to_str().unwrap()));
        let trash = NSFileManager::defaultManager()
            .URLForDirectory_inDomain_appropriateForURL_create_error(
                NSSearchPathDirectory::TrashDirectory,
                NSSearchPathDomainMask::UserDomainMask,
                Some(&source),
                false,
            )
            .expect("resolve the fixture volume's Trash directory");
        let recycled = PathBuf::from(trash.path().unwrap().to_string()).join(&name);
        assert_eq!(
            recycled.symlink_metadata().unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
        Self {
            source: root.join(&name),
            name,
            recycled,
            recovery: tempfile::Builder::new()
                .prefix("trash-recovery-")
                .tempdir_in(root)
                .unwrap(),
        }
    }

    pub fn recover(&self) -> PathBuf {
        let path = self.recovery.path().join("file");
        assert!(!path.exists());
        fs::rename(&self.recycled, &path).expect("recover isolated fixture from Trash");
        path
    }
}

impl Drop for Entry {
    fn drop(&mut self) {
        if self.recycled.symlink_metadata().is_ok() {
            let destination = self.recovery.path().join("file");
            if !destination.exists()
                && let Err(error) = fs::rename(&self.recycled, destination)
            {
                eprintln!("isolated fixture remains in Trash: {error}");
            }
        }
    }
}
