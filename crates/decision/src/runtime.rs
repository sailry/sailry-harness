//! Pinned native loading beside the executable; no system or environment lookup.
use crate::{Error, Result, ffi};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::{CString, c_void},
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
mod cpu;

const LOCK: &str = include_str!("../../../scripts/package/llama-runtime.lock.json");
static CURRENT: Mutex<Option<Arc<Runtime>>> = Mutex::new(None);

pub(crate) struct Runtime {
    pub api: ffi::Llama,
    _backend: ffi::backend::Backend,
    _base: ffi::base::Base,
}

#[derive(Deserialize)]
struct Lock {
    version: u32,
    build: String,
    runtime_version: String,
    source: String,
    targets: BTreeMap<String, Target>,
}
#[derive(Deserialize)]
struct Target {
    available: bool,
    archive: String,
    sha256: String,
    entry: Option<String>,
    files: Vec<Pinned>,
}
#[derive(Deserialize)]
struct Pinned {
    name: String,
    size: u64,
    sha256: String,
}
#[derive(Deserialize)]
struct Manifest {
    version: u32,
    target: String,
    build: String,
    runtime_version: String,
    available: bool,
    entry: Option<String>,
    source: Source,
    files: Vec<Installed>,
}
#[derive(Deserialize)]
struct Source {
    url: String,
    sha256: String,
}
#[derive(Deserialize)]
struct Installed {
    name: String,
    size: u64,
    source_sha256: String,
    sha256: String,
}

struct Artifact {
    entry: String,
    variants: Vec<String>,
}

pub(crate) fn initialize() -> Result<Arc<Runtime>> {
    let mut current = CURRENT.lock().unwrap();
    if let Some(runtime) = current.as_ref() {
        return Ok(runtime.clone());
    }
    let executable = std::env::current_exe()?;
    let directory = executable
        .parent()
        .ok_or(Error::Unavailable("executable directory is missing"))?;
    let runtime = Arc::new(load(directory)?);
    *current = Some(runtime.clone());
    Ok(runtime)
}

pub(crate) fn available() -> Result<()> {
    let executable = std::env::current_exe()?;
    let directory = executable
        .parent()
        .ok_or(Error::Unavailable("executable directory is missing"))?;
    verify(directory).map(|_| ())
}

fn target() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("windows", "aarch64") => Ok("aarch64-pc-windows-msvc"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        _ => Err(Error::Unavailable(
            "platform has no supported prebuilt runtime",
        )),
    }
}

fn platform() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let mut bytes = [0u8; 128];
        let mut length = bytes.len();
        // SAFETY: sysctl receives a bounded writable buffer and its actual length.
        let status = unsafe {
            libc::sysctlbyname(
                c"kern.osproductversion".as_ptr(),
                bytes.as_mut_ptr().cast(),
                &mut length,
                std::ptr::null_mut(),
                0,
            )
        };
        if status != 0 || length == 0 || length > bytes.len() {
            return Err(Error::Unavailable("macOS version could not be read"));
        }
        let version = std::str::from_utf8(&bytes[..length])
            .map_err(|_| Error::Unavailable("macOS version is invalid"))?;
        if !at_least(version.trim_end_matches('\0'), (13, 3)) {
            return Err(Error::Unavailable("macOS 13.3 or newer is required"));
        }
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
    {
        // SAFETY: glibc returns a static nul-terminated version string.
        let version = unsafe { std::ffi::CStr::from_ptr(libc::gnu_get_libc_version()) };
        if !version.to_str().is_ok_and(|value| at_least(value, (2, 34))) {
            return Err(Error::Unavailable("glibc 2.34 or newer is required"));
        }
    }
    #[cfg(all(target_os = "linux", not(target_env = "gnu")))]
    return Err(Error::Unavailable("a glibc runtime is required"));
    Ok(())
}

fn at_least(value: &str, minimum: (u32, u32)) -> bool {
    let mut parts = value.split('.');
    let version = parts.next().and_then(|major| {
        Some((
            major.parse::<u32>().ok()?,
            parts.next()?.parse::<u32>().ok()?,
        ))
    });
    version.is_some_and(|version| version >= minimum)
}

fn ordinary(path: &Path) -> Result<PathBuf> {
    if !path.symlink_metadata()?.is_file() {
        return Err(Error::Unavailable(
            "native runtime must contain ordinary files",
        ));
    }
    Ok(path.canonicalize()?)
}

fn manifest(directory: &Path) -> PathBuf {
    if cfg!(target_os = "macos")
        && directory.file_name().is_some_and(|name| name == "MacOS")
        && let Some(contents) = directory
            .parent()
            .filter(|path| path.file_name().is_some_and(|name| name == "Contents"))
    {
        return contents.join("Resources/llama-runtime.json");
    }
    directory.join("llama-runtime.json")
}

fn verify(directory: &Path) -> Result<Artifact> {
    platform()?;
    let identity = target()?;
    let lock: Lock = serde_json::from_str(LOCK)
        .map_err(|_| Error::Unavailable("packaged runtime lock is invalid"))?;
    let target = lock
        .targets
        .get(identity)
        .filter(|target| target.available)
        .ok_or(Error::Unavailable(
            "runtime is unavailable on this platform",
        ))?;
    let path = ordinary(&manifest(directory))?;
    let file = File::open(path)?;
    if file.metadata()?.len() > 128 * 1024 {
        return Err(Error::Unavailable(
            "native runtime manifest exceeds its limit",
        ));
    }
    let installed: Manifest = serde_json::from_reader(file)
        .map_err(|_| Error::Unavailable("native runtime manifest is invalid"))?;
    if lock.version != 1
        || installed.version != 1
        || installed.target != identity
        || installed.build != lock.build
        || installed.runtime_version != lock.runtime_version
        || !installed.available
        || installed.entry != target.entry
        || installed.files.len() != target.files.len()
        || installed.source.url != format!("{}{}", lock.source, target.archive)
        || installed.source.sha256 != target.sha256
    {
        return Err(Error::Unavailable("native runtime does not match its lock"));
    }
    for (file, pinned) in installed.files.iter().zip(&target.files) {
        if file.name != pinned.name
            || file.source_sha256 != pinned.sha256
            || (!cfg!(target_os = "macos")
                && (file.size != pinned.size || file.sha256 != pinned.sha256))
        {
            return Err(Error::Unavailable("native runtime provenance is invalid"));
        }
        let path = ordinary(&directory.join(&pinned.name))?;
        let mut input = File::open(path)?;
        if input.metadata()?.len() != file.size {
            return Err(Error::Unavailable(
                "native runtime file size does not match",
            ));
        }
        let mut hash = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        loop {
            let count = input.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        if format!("{:x}", hash.finalize()) != file.sha256 {
            return Err(Error::Unavailable("native runtime checksum does not match"));
        }
    }
    let entry = target
        .entry
        .clone()
        .ok_or(Error::Unavailable("native runtime entry is missing"))?;
    Ok(Artifact {
        entry,
        variants: target
            .files
            .iter()
            .filter(|file| cpu::variant(&file.name))
            .map(|file| file.name.clone())
            .collect(),
    })
}

fn names() -> Result<(&'static str, &'static str, Option<&'static str>)> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", _) => Ok((
            "libggml-base.0.dylib",
            "libggml.0.dylib",
            Some("libggml-cpu.0.dylib"),
        )),
        ("linux", "x86_64") => Ok(("libggml-base.so.0", "libggml.so.0", None)),
        ("windows", "x86_64") => Ok(("ggml-base.dll", "ggml.dll", None)),
        ("windows", "aarch64") => Ok(("ggml-base.dll", "ggml.dll", Some("ggml-cpu.dll"))),
        _ => Err(Error::Unavailable("CPU backend is unavailable")),
    }
}

fn load(directory: &Path) -> Result<Runtime> {
    let artifact = verify(directory)?;
    let (base, backend, cpu) = names()?;
    // SAFETY: the base library and its callback signature share the same pinned tag.
    let base = unsafe { ffi::base::Base::from_library(library(&directory.join(base))?) }?;
    // SAFETY: every dependency was verified against the packaged b11756 manifest.
    let backend =
        unsafe { ffi::backend::Backend::from_library(library(&directory.join(backend))?) }?;
    // SAFETY: the required public C symbols and by-value layouts are pinned together.
    let api = unsafe { ffi::Llama::from_library(library(&directory.join(artifact.entry))?) }?;
    // SAFETY: the callback captures no data and remains valid for process lifetime.
    unsafe {
        api.llama_log_set(Some(discard), std::ptr::null_mut());
        base.ggml_log_set(Some(discard), std::ptr::null_mut());
    }
    // macOS links the CPU registry into ggml. Other packages ship dynamic variants.
    // Query first; load only the exact CPU member if it is not already registered.
    // SAFETY: the public API selects only a registered CPU device.
    let mut device = unsafe {
        backend.ggml_backend_dev_by_type(
            ffi::backend::ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_CPU,
        )
    };
    if device.is_null() {
        let path = if artifact.variants.is_empty() {
            ordinary(
                &directory.join(cpu.ok_or(Error::Unavailable("CPU backend variants are missing"))?),
            )?
        } else {
            cpu::select(directory, &artifact.variants)?
        };
        let path = CString::new(path.as_os_str().as_encoded_bytes())
            .map_err(|_| Error::Unavailable("CPU backend path is invalid"))?;
        // SAFETY: this is one exact, verified CPU library, never a directory scan.
        if unsafe { backend.ggml_backend_load(path.as_ptr()) }.is_null() {
            return Err(Error::Unavailable("CPU backend could not be loaded"));
        }
        // SAFETY: the explicit backend load just registered its devices.
        device = unsafe {
            backend.ggml_backend_dev_by_type(
                ffi::backend::ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_CPU,
            )
        };
    }
    if device.is_null() {
        return Err(Error::Unavailable("CPU backend has no device"));
    }
    // SAFETY: the CPU is already registered, so b11756's initializer will not take
    // its implicit load_all fallback. It initializes the shared floating-point tables.
    unsafe {
        api.llama_backend_init();
    }
    Ok(Runtime {
        api,
        _backend: backend,
        _base: base,
    })
}

fn library(path: &Path) -> Result<libloading::Library> {
    let path = ordinary(path)?;
    #[cfg(windows)]
    {
        use libloading::os::windows::{
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, Library,
        };
        // SAFETY: load only the verified absolute file and its colocated or OS dependencies.
        let library = unsafe {
            Library::load_with_flags(
                path,
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        }?;
        Ok(library.into())
    }
    #[cfg(not(windows))]
    {
        // SAFETY: the ordinary file was verified against the packaged b11756 manifest.
        Ok(unsafe { libloading::Library::new(path) }?)
    }
}

unsafe extern "C" fn discard(_: ffi::ggml_log_level, _: *const std::ffi::c_char, _: *mut c_void) {}

#[cfg(test)]
mod tests;
