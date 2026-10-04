use std::{
    fs::OpenOptions,
    io::{Read, Write},
    path::Path,
};

use iroh::SecretKey;
use sailry_protocol::NodeId;

/// Loaded only by the profile owner. No Debug implementation may reveal the secret.
pub struct Identity(pub(crate) SecretKey);

impl Identity {
    pub fn open(directory: &Path) -> std::io::Result<Self> {
        let path = directory.join("link.key");
        let key = SecretKey::generate();
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(mut file) => {
                file.write_all(&key.to_bytes())?;
                file.sync_all()?;
                #[cfg(unix)]
                std::fs::File::open(directory)?.sync_all()?;
                Ok(Self(key))
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = path.symlink_metadata()?;
                if !metadata.is_file() || metadata.is_symlink() {
                    return Err(std::io::Error::other(
                        "identity must be a regular private file",
                    ));
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if metadata.permissions().mode() & 0o077 != 0 {
                        return Err(std::io::Error::other(
                            "identity file permissions must exclude group and other users",
                        ));
                    }
                }
                let mut bytes = Vec::new();
                std::fs::File::open(&path)?
                    .take(33)
                    .read_to_end(&mut bytes)?;
                let bytes: [u8; 32] = bytes.try_into().map_err(|_| {
                    std::io::Error::other(
                        "invalid identity file length; refusing identity replacement",
                    )
                })?;
                Ok(Self(SecretKey::from_bytes(&bytes)))
            }
            Err(error) => Err(error),
        }
    }

    pub fn id(&self) -> NodeId {
        NodeId(*self.0.public().as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reload_preserves_public_identity() {
        let directory = tempfile::tempdir().unwrap();
        let first = Identity::open(directory.path()).unwrap();
        let second = Identity::open(directory.path()).unwrap();
        assert_eq!(first.id(), second.id());
    }

    #[test]
    fn corrupt_identity_is_never_replaced() {
        let directory = tempfile::tempdir().unwrap();
        Identity::open(directory.path()).unwrap();
        std::fs::write(directory.path().join("link.key"), b"truncated").unwrap();
        assert!(Identity::open(directory.path()).is_err());
        assert_eq!(
            std::fs::read(directory.path().join("link.key")).unwrap(),
            b"truncated"
        );
    }
}
