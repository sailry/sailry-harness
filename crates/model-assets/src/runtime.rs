//! Locate the packaged native runtime without consulting process search paths.
use std::{
    io,
    path::{Path, PathBuf},
};

pub const VERSION: &str = "1.28.2";

#[cfg(feature = "onnx")]
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("ONNX Runtime file is unavailable: {0}")]
    Io(#[from] io::Error),
    #[error("ONNX Runtime library is invalid: {0}")]
    Library(#[from] libloading::Error),
    #[error("ONNX Runtime could not load: {0}")]
    LoadDynamic(#[from] ort::LoadDynamicError),
    #[error("ONNX Runtime is invalid: {0}")]
    InvalidRuntime(&'static str),
}

/// Initializes the same packaged runtime for every native ONNX consumer.
#[cfg(feature = "onnx")]
pub fn initialize() -> Result<PathBuf, RuntimeError> {
    let path = path()?;
    // SAFETY: path() returns the canonical packaged, pinned native runtime file.
    let library = unsafe { libloading::Library::new(&path) }?;
    // SAFETY: ONNX Runtime exports the typed ort-sys base API getter.
    let getter = unsafe {
        library
            .get::<unsafe extern "system" fn() -> *const ort::sys::OrtApiBase>(b"OrtGetApiBase\0")
    }?;
    // SAFETY: the symbol remains valid while its library is owned by this scope.
    let base = unsafe { getter() };
    // SAFETY: the C API returns a static table; null is rejected below.
    let base =
        unsafe { base.as_ref() }.ok_or(RuntimeError::InvalidRuntime("base API is unavailable"))?;
    // SAFETY: GetVersionString belongs to the typed base API table.
    let version = unsafe { (base.GetVersionString)() };
    if version.is_null() {
        return Err(RuntimeError::InvalidRuntime("version is unavailable"));
    }
    // SAFETY: GetVersionString returns a static, nul-terminated version string.
    let version = unsafe { std::ffi::CStr::from_ptr(version) }
        .to_str()
        .map_err(|_| RuntimeError::InvalidRuntime("version is not valid UTF-8"))?;
    validate_version(version)?;
    // SAFETY: this version is checked before requesting the pinned ort-sys API.
    let expected = unsafe { (base.GetApi)(ort::sys::ORT_API_VERSION) };
    if expected.is_null() {
        return Err(RuntimeError::InvalidRuntime("required API is unavailable"));
    }
    ort::init_from(&path)?.with_name("Sailry").commit();
    if expected != std::ptr::from_ref(ort::api()) {
        return Err(RuntimeError::InvalidRuntime(
            "another runtime is already active",
        ));
    }
    Ok(path)
}

#[cfg(feature = "onnx")]
fn validate_version(version: &str) -> Result<(), RuntimeError> {
    if version != VERSION {
        return Err(RuntimeError::InvalidRuntime("version must be 1.28.2"));
    }
    Ok(())
}

pub fn path() -> io::Result<PathBuf> {
    beside(&std::env::current_exe()?)
}

fn name() -> io::Result<&'static str> {
    match std::env::consts::OS {
        "macos" => Ok("libonnxruntime.dylib"),
        "linux" => Ok("libonnxruntime.so"),
        "windows" => Ok("onnxruntime.dll"),
        _ => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "ONNX Runtime is unavailable on this platform",
        )),
    }
}

fn beside(executable: &Path) -> io::Result<PathBuf> {
    let directory = executable.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "executable directory is unavailable",
        )
    })?;
    let library = directory.join(name()?);
    if !library.symlink_metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ONNX Runtime library is not an ordinary file",
        ));
    }
    library.canonicalize()
}

#[cfg(all(
    test,
    any(target_os = "macos", target_os = "linux", target_os = "windows")
))]
mod tests {
    use super::*;

    #[cfg(feature = "onnx")]
    #[test]
    fn accepts_only_pinned_version() {
        validate_version("1.28.2").unwrap();
        for version in ["", "1.28.1", "1.29.0", "1.28.2-local", "1.28.2\n"] {
            assert!(matches!(
                validate_version(version),
                Err(RuntimeError::InvalidRuntime(_))
            ));
        }
    }

    #[test]
    fn resolves_beside_binary() {
        let directory = tempfile::tempdir().unwrap();
        let library = directory.path().join(name().unwrap());
        std::fs::write(&library, b"fixture").unwrap();
        assert_eq!(
            beside(&directory.path().join("binary")).unwrap(),
            library.canonicalize().unwrap()
        );
    }

    #[test]
    fn rejects_missing_library() {
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(
            beside(&directory.path().join("binary")).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }

    #[test]
    fn rejects_directory() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join(name().unwrap())).unwrap();
        assert_eq!(
            beside(&directory.path().join("binary")).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink() {
        let directory = tempfile::tempdir().unwrap();
        let other = directory.path().join("other");
        std::fs::write(&other, b"fixture").unwrap();
        std::os::unix::fs::symlink(&other, directory.path().join(name().unwrap())).unwrap();
        assert_eq!(
            beside(&directory.path().join("binary")).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
