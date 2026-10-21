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
// Successful native images and this executable's compiled lock stay immutable until restart.
static CURRENT: Mutex<Option<Arc<Runtime>>> = Mutex::new(None);
// Verified packaged files are reused when a later dynamic load is retried.
static VERIFIED: Mutex<Option<Arc<Artifact>>> = Mutex::new(None);

pub(crate) struct Runtime {
    pub api: ffi::Llama,
    _backend: ffi::backend::Backend,
    _base: ffi::base::Base,
    _cpu: Option<libloading::Library>,
    cpu: Option<CString>,
    started: Mutex<Started>,
}

/// Native registration steps that must not repeat after a later step fails.
#[derive(Default)]
struct Started {
    backend: bool,
    initialized: bool,
}

#[derive(Deserialize)]
struct Lock {
    version: u32,
    build: String,
    targets: BTreeMap<String, Target>,
}
#[derive(Deserialize)]
struct Target {
    available: bool,
    entry: Option<String>,
    minimum_macos: Option<String>,
    minimum_glibc: Option<String>,
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
    files: Vec<Installed>,
}
#[derive(Deserialize)]
struct Installed {
    name: String,
    size: u64,
    sha256: String,
}

struct Artifact {
    entry: String,
    variants: Vec<String>,
}

pub(crate) fn initialize() -> Result<Arc<Runtime>> {
    let runtime = prepared()?;
    runtime.start()?;
    Ok(runtime)
}

pub(crate) fn available() -> Result<()> {
    prepared().map(|_| ())
}

fn prepared() -> Result<Arc<Runtime>> {
    cached(&CURRENT, || {
        let executable = std::env::current_exe()?;
        let directory = executable
            .parent()
            .ok_or_else(|| Error::Unavailable("executable directory is missing".into()))?;
        open(directory, &VERIFIED)
    })
}

fn open(directory: &Path, verified: &Mutex<Option<Arc<Artifact>>>) -> Result<Runtime> {
    let artifact = cached(verified, || verify(directory))?;
    load(directory, &artifact)
}

fn cached<T>(cache: &Mutex<Option<Arc<T>>>, load: impl FnOnce() -> Result<T>) -> Result<Arc<T>> {
    let mut cache = cache.lock().expect("native preflight lock is healthy");
    if let Some(value) = cache.as_ref() {
        return Ok(value.clone());
    }
    let value = Arc::new(load().map_err(|error| Error::Unavailable(reason(error).into()))?);
    *cache = Some(value.clone());
    Ok(value)
}

fn reason(error: Error) -> String {
    match error {
        Error::Unavailable(message) => message.into_owned(),
        Error::Library(error) => {
            let mut message = error.to_string();
            let mut source = std::error::Error::source(&error);
            while let Some(error) = source {
                message.push_str(": ");
                message.push_str(&error.to_string());
                source = error.source();
            }
            message
        }
        error => error.to_string(),
    }
}

fn target() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        ("windows", "aarch64") => Ok("aarch64-pc-windows-msvc"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        _ => Err(Error::Unavailable(
            "platform has no supported prebuilt runtime".into(),
        )),
    }
}

fn platform(asset: &Target) -> Result<()> {
    let _minimum = match std::env::consts::OS {
        "macos" => asset.minimum_macos.as_deref(),
        "linux" => asset.minimum_glibc.as_deref(),
        _ => None,
    }
    .map(|value| {
        version(value)
            .ok_or_else(|| Error::Unavailable("native platform minimum is invalid".into()))
    })
    .transpose()?;
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
            return Err(Error::Unavailable("macOS version could not be read".into()));
        }
        let version = std::str::from_utf8(&bytes[..length])
            .map_err(|_| Error::Unavailable("macOS version is invalid".into()))?;
        let minimum =
            _minimum.ok_or_else(|| Error::Unavailable("macOS minimum is missing".into()))?;
        if !at_least(version.trim_end_matches('\0'), minimum) {
            return Err(Error::Unavailable(
                format!("macOS {}.{} or newer is required", minimum.0, minimum.1).into(),
            ));
        }
    }
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        // SAFETY: glibc returns a static nul-terminated version string.
        let version = unsafe { std::ffi::CStr::from_ptr(libc::gnu_get_libc_version()) };
        let minimum =
            _minimum.ok_or_else(|| Error::Unavailable("glibc minimum is missing".into()))?;
        if !version.to_str().is_ok_and(|value| at_least(value, minimum)) {
            return Err(Error::Unavailable(
                format!("glibc {}.{} or newer is required", minimum.0, minimum.1).into(),
            ));
        }
    }
    #[cfg(all(target_os = "linux", not(target_env = "gnu")))]
    return Err(Error::Unavailable("a glibc runtime is required".into()));
    Ok(())
}

fn version(value: &str) -> Option<(u32, u32)> {
    let mut parts = value.split('.');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

fn at_least(value: &str, minimum: (u32, u32)) -> bool {
    version(value).is_some_and(|value| value >= minimum)
}

fn ordinary(path: &Path) -> Result<PathBuf> {
    if !path.symlink_metadata()?.is_file() {
        return Err(Error::Unavailable(
            "native runtime must contain ordinary files".into(),
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
    let identity = target()?;
    let lock: Lock = serde_json::from_str(LOCK)
        .map_err(|_| Error::Unavailable("packaged runtime lock is invalid".into()))?;
    let target = lock
        .targets
        .get(identity)
        .filter(|target| target.available)
        .ok_or(Error::Unavailable(
            "runtime is unavailable on this platform".into(),
        ))?;
    platform(target)?;
    let path = ordinary(&manifest(directory))?;
    let file = File::open(path)?;
    if file.metadata()?.len() > 128 * 1024 {
        return Err(Error::Unavailable(
            "native runtime manifest exceeds its limit".into(),
        ));
    }
    let installed: Manifest = serde_json::from_reader(file)
        .map_err(|_| Error::Unavailable("native runtime manifest is invalid".into()))?;
    if lock.version != 1
        || installed.version != 1
        || installed.target != identity
        || installed.build != lock.build
        || installed.files.len() != target.files.len()
    {
        return Err(Error::Unavailable(
            "native runtime does not match its lock".into(),
        ));
    }
    for (file, pinned) in installed.files.iter().zip(&target.files) {
        if file.name != pinned.name
            || (!cfg!(target_os = "macos")
                && (file.size != pinned.size || file.sha256 != pinned.sha256))
        {
            return Err(Error::Unavailable(
                "native runtime provenance is invalid".into(),
            ));
        }
        let path = ordinary(&directory.join(&pinned.name))?;
        let mut input = File::open(path)?;
        if input.metadata()?.len() != file.size {
            return Err(Error::Unavailable(
                "native runtime file size does not match".into(),
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
            return Err(Error::Unavailable(
                "native runtime checksum does not match".into(),
            ));
        }
    }
    let entry = target
        .entry
        .clone()
        .ok_or(Error::Unavailable("native runtime entry is missing".into()))?;
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
        ("linux", "x86_64" | "aarch64") => Ok(("libggml-base.so.0", "libggml.so.0", None)),
        ("windows", "x86_64") => Ok(("ggml-base.dll", "ggml.dll", None)),
        ("windows", "aarch64") => Ok(("ggml-base.dll", "ggml.dll", Some("ggml-cpu.dll"))),
        _ => Err(Error::Unavailable("CPU backend is unavailable".into())),
    }
}

fn load(directory: &Path, artifact: &Artifact) -> Result<Runtime> {
    let (base, backend, cpu) = names()?;
    // SAFETY: the base library and its callback signature share the same pinned tag.
    let base = unsafe { ffi::base::Base::from_library(library(&directory.join(base))?) }?;
    // SAFETY: every dependency was verified against the packaged b11756 manifest.
    let backend =
        unsafe { ffi::backend::Backend::from_library(library(&directory.join(backend))?) }?;
    // SAFETY: the required public C symbols and by-value layouts are pinned together.
    let api = unsafe { ffi::Llama::from_library(library(&directory.join(&artifact.entry))?) }?;
    // SAFETY: the callback captures no data and remains valid for process lifetime.
    unsafe {
        api.llama_log_set(Some(discard), std::ptr::null_mut());
        base.ggml_log_set(Some(discard), std::ptr::null_mut());
    }
    // Preflight dynamic dependencies and select the immutable CPU image. Model
    // loading and backend initialization stay on the existing inference worker.
    // SAFETY: this query does not create a backend or model.
    let device = unsafe {
        backend.ggml_backend_dev_by_type(
            ffi::backend::ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_CPU,
        )
    };
    let (cpu, cpu_library) = if device.is_null() {
        let path = if artifact.variants.is_empty() {
            ordinary(&directory.join(
                cpu.ok_or_else(|| Error::Unavailable("CPU backend variants are missing".into()))?,
            ))?
        } else {
            cpu::select(directory, &artifact.variants)?
        };
        let cpu_library = library(&path)?;
        let path = CString::new(path.as_os_str().as_encoded_bytes())
            .map_err(|_| Error::Unavailable("CPU backend path is invalid".into()))?;
        (Some(path), Some(cpu_library))
    } else {
        (None, None)
    };
    Ok(Runtime {
        api,
        _backend: backend,
        _base: base,
        _cpu: cpu_library,
        cpu,
        started: Mutex::new(Started::default()),
    })
}

impl Runtime {
    fn start(&self) -> Result<()> {
        let mut started = self
            .started
            .lock()
            .expect("native initialization lock is healthy");
        if started.initialized {
            return Ok(());
        }
        if let Some(path) = self.cpu.as_ref().filter(|_| !started.backend) {
            // SAFETY: preflight retained this exact locked CPU image and dependencies.
            if unsafe { self._backend.ggml_backend_load(path.as_ptr()) }.is_null() {
                return Err(Error::Unavailable("CPU backend could not be loaded".into()));
            }
            started.backend = true;
        }
        // SAFETY: CPU registration is complete before initializing floating-point tables.
        let device = unsafe {
            self._backend.ggml_backend_dev_by_type(
                ffi::backend::ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_CPU,
            )
        };
        if device.is_null() {
            return Err(Error::Unavailable("CPU backend has no device".into()));
        }
        // SAFETY: a registered CPU prevents the initializer's implicit directory scan.
        unsafe { self.api.llama_backend_init() };
        started.initialized = true;
        Ok(())
    }
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
