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
}

#[test]
fn rejects_unpinned_manifest_before_loading() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("llama-runtime.json"), serde_json::json!({
        "version":1,"target":target().unwrap(),"build":"different","runtime_version":"0.7.0",
        "available":true,"entry":"libllama.0.dylib","source":{"url":"fixture","sha256":"fixture"},"files":[]
    }).to_string()).unwrap();
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
    let runtime = initialize().unwrap();
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
