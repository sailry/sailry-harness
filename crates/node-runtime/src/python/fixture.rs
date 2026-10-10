//! Known-ready assets exercise lifecycle UI without running Python or downloading wheels.
use super::*;

impl Manager {
    pub(crate) fn ready_fixture(
        profile: PathBuf,
        requirements: &Requirements,
        environment: Arc<Environment>,
        progress: Arc<watch::Sender<Vec<sailry_protocol::python::Preparation>>>,
    ) -> Result<(Self, Arc<std::sync::atomic::AtomicUsize>), Fault> {
        let runtime = Runtime::fixture(&profile.join("python-fixture"))?;
        let mut manager = Self::with_progress(profile, runtime, environment, progress)?;
        // This asset represents a completed runtime probe. Its interpreter is
        // deliberately non-executable, so an unexpected preparation cannot run.
        manager.checked.set(()).map_err(storage)?;
        let key = requirements.key(&manager.runtime.identity);
        let root = manager.root.join("environments").join(&key);
        std::fs::create_dir(&root).map_err(storage)?;
        let python = runtime::python(&root);
        std::fs::create_dir_all(python.parent().expect("interpreter has a parent"))
            .map_err(storage)?;
        std::fs::copy(&manager.runtime.python, python).map_err(storage)?;
        std::fs::write(root.join(".sailry-requirements.txt"), &requirements.content)
            .map_err(storage)?;
        integrity::seal(
            &root,
            &manager.runtime.root,
            &key,
            &CancellationToken::new(),
        )?;
        let installations = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        manager.fixture_installations = Some(installations.clone());
        Ok((manager, installations))
    }
}
