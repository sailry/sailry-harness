//! Retained object identity adapted from sailry-code 67ae9fa0:
//! sailry-code-secure-file/src/secure/{identity,windows}.rs (Apache-2.0).
use cap_std::fs::{Dir, File};
#[cfg(not(unix))]
use sailry_protocol::ErrorCode;
use sailry_protocol::Fault;

use crate::files::io_error;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, serde::Serialize)]
pub(crate) struct Identity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(windows)]
    volume: u64,
    #[cfg(windows)]
    file: [u8; 16],
}

pub(crate) fn directory(dir: &Dir) -> Result<Identity, Fault> {
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt;
        let metadata = dir.dir_metadata().map_err(io_error)?;
        Ok(Identity {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        windows::checked(dir.as_raw_handle())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = dir;
        Err(Fault::new(
            ErrorCode::Unavailable,
            "filesystem identity is unsupported",
        ))
    }
}

pub(crate) fn file(file: &File) -> Result<Identity, Fault> {
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt;
        let metadata = file.metadata().map_err(io_error)?;
        Ok(Identity {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        windows::checked(file.as_raw_handle())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = file;
        Err(Fault::new(
            ErrorCode::Unavailable,
            "filesystem identity is unsupported",
        ))
    }
}

#[cfg(windows)]
pub(super) mod windows {
    use super::*;
    use std::{
        fs::OpenOptions,
        mem::{MaybeUninit, size_of},
        os::windows::{
            fs::OpenOptionsExt,
            io::{AsRawHandle, RawHandle},
        },
        path::Path,
    };
    use windows_sys::Win32::Storage::FileSystem::*;

    pub(in crate::files::path) fn open(path: &Path) -> Result<Dir, Fault> {
        // Preserve the old capability rule: directory handles deny delete
        // sharing, so an ancestor cannot be renamed while it is retained.
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .map_err(io_error)?;
        checked(file.as_raw_handle())?;
        if !file.metadata().map_err(io_error)?.is_dir() {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "expected a directory",
            ));
        }
        Ok(Dir::from_std_file(file))
    }

    pub(super) fn checked(handle: RawHandle) -> Result<Identity, Fault> {
        let mut tag = MaybeUninit::<FILE_ATTRIBUTE_TAG_INFO>::zeroed();
        // SAFETY: the caller borrows a live file/directory handle. The output
        // buffer is writable and sized for the requested information class.
        let result = unsafe {
            GetFileInformationByHandleEx(
                handle,
                FileAttributeTagInfo,
                tag.as_mut_ptr().cast(),
                size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
            )
        };
        if result == 0 {
            return Err(io_error(std::io::Error::last_os_error()));
        }
        // SAFETY: the API reported successful initialization of the structure.
        if unsafe { tag.assume_init() }.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "filesystem reparse points are not allowed",
            ));
        }
        let mut info = MaybeUninit::<FILE_ID_INFO>::zeroed();
        // SAFETY: the same live handle and a correctly sized output buffer.
        let result = unsafe {
            GetFileInformationByHandleEx(
                handle,
                FileIdInfo,
                info.as_mut_ptr().cast(),
                size_of::<FILE_ID_INFO>() as u32,
            )
        };
        if result == 0 {
            return Err(io_error(std::io::Error::last_os_error()));
        }
        // SAFETY: the API reported successful initialization of the structure.
        let info = unsafe { info.assume_init() };
        Ok(Identity {
            volume: info.VolumeSerialNumber,
            file: info.FileId.Identifier,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn retains_all_file_id_bits() {
            let first = Identity {
                volume: 7,
                file: [0; 16],
            };
            let mut second = first;
            second.file[15] = 1;
            assert_ne!(first, second);
        }

        #[test]
        fn retained_directory_denies_rename() {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("directory");
            std::fs::create_dir(&path).unwrap();
            let retained = open(&path).unwrap();
            assert!(std::fs::rename(&path, temp.path().join("moved")).is_err());
            drop(retained);
            std::fs::rename(&path, temp.path().join("moved")).unwrap();
        }
    }
}
