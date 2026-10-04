use sailry_link::CancellationToken;
use sailry_protocol::FILE_TRANSFER_CHUNK_BYTES;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek},
    path::Path,
};

pub(crate) struct Source {
    pub file: File,
    pub size: u64,
    pub revision: String,
}

impl Source {
    pub fn open(path: &Path, cancel: &CancellationToken) -> Result<Self, &'static str> {
        Self::open_limited(path, cancel, u64::MAX)
    }

    pub fn open_limited(
        path: &Path,
        cancel: &CancellationToken,
        limit: u64,
    ) -> Result<Self, &'static str> {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NONBLOCK);
        let mut file = options.open(path).map_err(|_| "files_upload_source")?;
        let before = file.metadata().map_err(|_| "files_upload_source")?;
        if !before.is_file() {
            return Err("files_upload_source");
        }
        let size = before.len();
        if size > limit {
            return Err("files_upload_too_large");
        }
        let mut read = 0u64;
        let mut hash = blake3::Hasher::new();
        let mut bytes = [0; FILE_TRANSFER_CHUNK_BYTES];
        loop {
            if cancel.is_cancelled() {
                return Err("files_upload_cancelled");
            }
            let count = file.read(&mut bytes).map_err(|_| "files_upload_source")?;
            if count == 0 {
                break;
            }
            read = read
                .checked_add(count as u64)
                .filter(|read| *read <= size)
                .ok_or("files_upload_changed")?;
            hash.update(&bytes[..count]);
        }
        let after = file.metadata().map_err(|_| "files_upload_source")?;
        if read != size || after.len() != size || before.modified().ok() != after.modified().ok() {
            return Err("files_upload_changed");
        }
        file.rewind().map_err(|_| "files_upload_source")?;
        Ok(Self {
            file,
            size,
            revision: hash.finalize().to_hex().to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_binary_files() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("资料.bin");
        for data in [vec![0xf1; 2 * 1024 * 1024], vec![]] {
            std::fs::write(&path, &data).unwrap();
            let mut source = Source::open(&path, &CancellationToken::new()).unwrap();
            assert_eq!(source.size, data.len() as u64);
            assert_eq!(source.revision, blake3::hash(&data).to_hex().as_str());
            let mut observed = Vec::new();
            source.file.read_to_end(&mut observed).unwrap();
            assert_eq!(observed, data);
        }
        assert!(Source::open(temp.path(), &CancellationToken::new()).is_err());
        std::fs::write(&path, [1; 16]).unwrap();
        assert!(matches!(
            Source::open_limited(&path, &CancellationToken::new(), 8),
            Err("files_upload_too_large")
        ));
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(matches!(
            Source::open(&path, &cancel),
            Err("files_upload_cancelled")
        ));
    }
}
