//! MCP needs a named file, while cap-tempfile deliberately exposes no temporary name.
//! Keep creation and cleanup anchored to the retained data directory, not its ambient path.
use super::*;
use std::io::{self, Write};

pub(crate) struct Projection {
    directory: Dir,
    name: String,
    removed: bool,
}

impl Projection {
    pub(super) fn create(
        directory: &Dir,
        values: &BTreeMap<String, serde_json::Value>,
    ) -> Result<Self, Fault> {
        let name = format!(".sailry-settings-{}.json", uuid::Uuid::new_v4());
        let directory = directory.try_clone().map_err(io_error)?;
        let mut options = cap_std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = directory.open_with(&name, &options).map_err(io_error)?;
        let projection = Self {
            directory,
            name,
            removed: false,
        };
        let bytes = serde_json::to_vec(values).map_err(|_| unavailable())?;
        file.write_all(&bytes)
            .and_then(|_| file.flush())
            .map_err(io_error)?;
        Ok(projection)
    }

    pub(super) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn close(mut self) -> io::Result<()> {
        match self.directory.remove_file(&self.name) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        self.removed = true;
        Ok(())
    }
}

impl Drop for Projection {
    fn drop(&mut self) {
        if !self.removed {
            let _ = self.directory.remove_file(&self.name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn releases_owned_projection() {
        let temp = tempfile::tempdir().unwrap();
        let root = Dir::open_ambient_dir(temp.path(), cap_std::ambient_authority()).unwrap();
        let values = BTreeMap::from([("label".into(), serde_json::json!("中文🙂"))]);
        let first = Projection::create(&root, &values).unwrap();
        let second = Projection::create(&root, &values).unwrap();
        let (one, two) = (first.name().to_owned(), second.name().to_owned());
        assert_ne!(one, two);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&root.read(&one).unwrap()).unwrap(),
            serde_json::json!({"label":"中文🙂"})
        );
        first.close().unwrap();
        assert!(!root.exists(&one));
        assert!(root.exists(&two));
        drop(second);
        assert!(!root.exists(&two));
    }

    #[cfg(unix)]
    #[test]
    fn retains_directory_identity() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("data")).unwrap();
        std::fs::create_dir(temp.path().join("outside")).unwrap();
        let root =
            Dir::open_ambient_dir(temp.path().join("data"), cap_std::ambient_authority()).unwrap();
        std::fs::rename(temp.path().join("data"), temp.path().join("retained")).unwrap();
        std::os::unix::fs::symlink(temp.path().join("outside"), temp.path().join("data")).unwrap();
        let projection = Projection::create(&root, &BTreeMap::new()).unwrap();
        let name = projection.name().to_owned();
        assert!(temp.path().join("retained").join(&name).exists());
        assert!(!temp.path().join("outside").join(&name).exists());
        projection.close().unwrap();
        assert!(!temp.path().join("retained").join(&name).exists());
    }
}
