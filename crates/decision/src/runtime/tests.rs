use super::*;

#[test]
fn compares_platform_baselines() {
    for version in ["13.3", "13.3.1", "14.0"] {
        assert!(at_least(version, (13, 3)));
    }
    for version in ["13.2", "12.7", "invalid", "13"] {
        assert!(!at_least(version, (13, 3)));
    }
    assert!(at_least("2.34", (2, 34)));
    assert!(!at_least("2.33", (2, 34)));
    assert!(!at_least("2.34", (2, 38)));
    assert!(at_least("2.38", (2, 38)));
    assert!(at_least("2.39", (2, 38)));
}

#[test]
fn rejects_unpinned_manifest_before_loading() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("llama-runtime.json"),
        serde_json::json!({
            "version":1,"target":target().unwrap(),"build":"different","files":[]
        })
        .to_string(),
    )
    .unwrap();
    assert!(matches!(
        verify(directory.path()),
        Err(Error::Unavailable(_))
    ));
}

#[cfg(target_os = "macos")]
#[test]
fn locates_bundle_resource_manifest() {
    assert_eq!(
        manifest(Path::new("/app/Contents/MacOS")),
        Path::new("/app/Contents/Resources/llama-runtime.json")
    );
    assert_eq!(
        manifest(Path::new("/bin")),
        Path::new("/bin/llama-runtime.json")
    );
}

#[cfg(unix)]
#[test]
fn rejects_nonordinary_runtime_paths() {
    let directory = tempfile::tempdir().unwrap();
    assert!(ordinary(directory.path()).is_err());
    let source = directory.path().join("source");
    std::fs::write(&source, b"fixture").unwrap();
    let link = directory.path().join("link");
    std::os::unix::fs::symlink(&source, &link).unwrap();
    assert!(matches!(ordinary(&link), Err(Error::Unavailable(_))));
}

#[test]
#[ignore = "requires the packaged b11756 CPU runtime beside the test executable"]
fn loads_scored_cpu_backend() {
    let executable = std::env::current_exe().unwrap();
    let directory = executable.parent().unwrap();
    let artifact = verify(directory).unwrap();
    let runtime = initialize().unwrap();
    if !artifact.variants.is_empty() {
        let selected = cpu::select(directory, &artifact.variants).unwrap();
        assert!(
            artifact
                .variants
                .iter()
                .any(|name| selected.file_name() == Some(std::ffi::OsStr::new(name)))
        );
        eprintln!(
            "Selected pinned CPU backend: {}",
            selected.file_name().unwrap().to_string_lossy()
        );
    }
    // SAFETY: the shared runtime retains the verified backend library and its registry.
    assert!(
        !unsafe {
            runtime._backend.ggml_backend_dev_by_type(
                ffi::backend::ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_CPU,
            )
        }
        .is_null()
    );
}

#[test]
fn retains_loader_cause() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("invalid-native-image");
    std::fs::write(&path, b"invalid native image fixture").unwrap();
    let error = library(&path).err().unwrap();
    let Error::Library(loader) = &error else {
        panic!("native loader error required");
    };
    let source = std::error::Error::source(loader).unwrap().to_string();
    assert!(!source.is_empty());
    assert!(reason(error).contains(&source));
}

#[test]
fn retries_package_failures() {
    let directory = tempfile::tempdir().unwrap();
    let cache = Mutex::new(None);
    let first = cached(&cache, || load(directory.path())).err().unwrap();
    assert!(matches!(first, Error::Unavailable(_)));
    std::fs::write(manifest(directory.path()), b"invalid manifest").unwrap();
    let mut inspected = false;
    let second = cached(&cache, || {
        inspected = true;
        load(directory.path())
    })
    .err()
    .unwrap();
    assert!(inspected);
    assert_ne!(second.to_string(), first.to_string());
    assert!(cache.lock().unwrap().is_none());
}

#[test]
#[ignore = "requires the packaged b11756 CPU runtime beside the test executable"]
fn caches_verified_package() {
    let executable = std::env::current_exe().unwrap();
    let source = executable.parent().unwrap();
    let original = manifest(source);
    let content = std::fs::read(&original).unwrap();
    let installed: Manifest = serde_json::from_slice(&content).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let cache = Mutex::new(None);
    assert!(cached(&cache, || load(directory.path())).is_err());
    assert!(cache.lock().unwrap().is_none());
    for file in installed.files {
        std::fs::copy(source.join(&file.name), directory.path().join(&file.name)).unwrap();
    }
    let manifest = manifest(directory.path());
    std::fs::write(&manifest, content).unwrap();
    let first = cached(&cache, || load(directory.path())).unwrap();
    assert!(!*first.initialized.lock().unwrap());
    std::fs::remove_file(&manifest).unwrap();
    let second = cached(&cache, || panic!("immutable package was checked twice")).unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    assert!(!*second.initialized.lock().unwrap());
    assert!(cached(&Mutex::new(None), || load(directory.path())).is_err());
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires a packaged runtime in an isolated Linux image without libgomp.so.1"]
fn reports_missing_dependency() {
    let first = available().unwrap_err();
    assert!(matches!(first, Error::Unavailable(_)));
    assert!(first.to_string().contains("libgomp.so.1"), "{first}");
    let second = available().unwrap_err();
    assert_eq!(second.to_string(), first.to_string());
}

#[cfg(all(target_os = "linux", target_arch = "aarch64", target_env = "gnu"))]
#[test]
#[ignore = "requires an isolated Linux ARM64 image older than glibc 2.38"]
fn rejects_old_glibc() {
    // SAFETY: glibc returns its static nul-terminated version string.
    let version = unsafe { std::ffi::CStr::from_ptr(libc::gnu_get_libc_version()) };
    assert!(!at_least(version.to_str().unwrap(), (2, 38)));
    let error = available().unwrap_err();
    assert!(matches!(error, Error::Unavailable(_)));
    assert_eq!(
        error.to_string(),
        "decision runtime is unavailable: glibc 2.38 or newer is required"
    );
}
