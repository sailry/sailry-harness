//! Known-ready assets exercise environment lifecycle without running Python or downloading wheels.
use super::*;

impl Manager {
    pub(crate) fn ready_fixture(
        profile: PathBuf,
        requirements: &Requirements,
        environment: Arc<Environment>,
        progress: Arc<watch::Sender<Vec<sailry_protocol::python::Preparation>>>,
    ) -> Result<(Self, Arc<std::sync::atomic::AtomicUsize>), Fault> {
        let runtime = profile.join("python-fixture");
        let runtime = match std::fs::symlink_metadata(&runtime) {
            Ok(_) => Runtime::open(&runtime)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Runtime::fixture(&runtime)?
            }
            Err(error) => return Err(storage(error)),
        };
        let mut manager = Self::with_progress(profile, runtime, environment, progress)?;
        // This asset represents a completed runtime probe. Its interpreter is
        // deliberately non-executable, so an unexpected preparation cannot run.
        manager.checked.set(()).map_err(storage)?;
        let key = requirements.key(&manager.runtime.identity);
        let root = manager.root.join("environments").join(&key);
        let stop = CancellationToken::new();
        match std::fs::create_dir(&root) {
            Ok(()) => {
                let python = runtime::python(&root);
                std::fs::create_dir_all(python.parent().expect("interpreter has a parent"))
                    .map_err(storage)?;
                std::fs::copy(&manager.runtime.python, python).map_err(storage)?;
                std::fs::write(root.join(".sailry-requirements.txt"), &requirements.content)
                    .map_err(storage)?;
                integrity::seal(&root, &manager.runtime.root, &key, &stop)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let mut validation = validation::Validation::new(root);
                if !validation.environment(&manager.runtime.root, &key, &stop)? {
                    return Err(Fault::invalid(
                        "existing Python fixture environment is invalid",
                    ));
                }
            }
            Err(error) => return Err(storage(error)),
        }
        let installations = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        manager.fixture_installations = Some(installations.clone());
        Ok((manager, installations))
    }
}
