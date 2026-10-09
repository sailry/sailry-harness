use super::{Manager, Requirements, Stage, check, integrity, process, runtime, storage};
use sailry_link::CancellationToken;
use sailry_protocol::Fault;
use std::path::Path;

impl Manager {
    pub(super) async fn prepare(
        &self,
        requirements: &Requirements,
        key: &str,
        destination: &Path,
        cancel: &CancellationToken,
    ) -> Result<(), Fault> {
        self.report(requirements, Stage::Creating);
        let stage = tempfile::Builder::new()
            .prefix("environment-")
            .tempdir_in(self.root.join("staging"))
            .map_err(storage)?;
        let variables = [
            (
                "PIP_CONFIG_FILE",
                if cfg!(windows) {
                    "NUL".into()
                } else {
                    "/dev/null".into()
                },
            ),
            ("PIP_CACHE_DIR", self.root.join("cache").into_os_string()),
        ];
        process::run(
            &self.runtime.python,
            &[
                "-I".into(),
                "-m".into(),
                "venv".into(),
                "--copies".into(),
                stage.path().to_string_lossy().into_owned(),
            ],
            stage.path(),
            &self.environment,
            &variables,
            cancel,
        )
        .await?;
        let python = runtime::python(stage.path());
        let requirements_path = stage.path().join(".sailry-requirements.txt");
        std::fs::write(&requirements_path, &requirements.content).map_err(storage)?;
        self.report(requirements, Stage::Installing);
        let mut args = vec![
            "-I".into(),
            "-m".into(),
            "pip".into(),
            "--isolated".into(),
            "install".into(),
            "--disable-pip-version-check".into(),
            "--no-input".into(),
            "--require-hashes".into(),
            "--only-binary=:all:".into(),
            "--no-deps".into(),
            "--no-compile".into(),
            "--cache-dir".into(),
            self.root.join("cache").to_string_lossy().into_owned(),
            "-r".into(),
            requirements_path.to_string_lossy().into_owned(),
        ];
        let index = self
            .index
            .lock()
            .expect("Python index lock is valid")
            .clone();
        if let Some(index) = index {
            args.extend(["--index-url".into(), index]);
        }
        process::run(
            &python,
            &args,
            stage.path(),
            &self.environment,
            &variables,
            cancel,
        )
        .await?;
        self.report(requirements, Stage::Validating);
        process::run(
            &python,
            &[
                "-I".into(),
                "-m".into(),
                "pip".into(),
                "--isolated".into(),
                "check".into(),
                "--disable-pip-version-check".into(),
            ],
            stage.path(),
            &self.environment,
            &variables,
            cancel,
        )
        .await?;
        // Registry entry points receive final-path launchers before the staging
        // directory moves. No activated shell or temporary shebang is retained.
        process::run(
            &python,
            &[
                "-I".into(),
                "-c".into(),
                include_str!("environment/relocate.py").into(),
                destination.to_string_lossy().into_owned(),
            ],
            stage.path(),
            &self.environment,
            &variables,
            cancel,
        )
        .await?;
        let path = stage.path().to_owned();
        let base = self.runtime.root.clone();
        let key = key.to_owned();
        let stop = cancel.clone();
        tokio::task::spawn_blocking(move || integrity::seal(&path, &base, &key, &stop))
            .await
            .map_err(|_| super::unavailable())??;
        check(cancel)?;
        if destination.symlink_metadata().is_ok() {
            let metadata = destination.symlink_metadata().map_err(storage)?;
            if !metadata.is_dir() || metadata.is_symlink() {
                return Err(Fault::invalid(
                    "Python environment destination is not a directory",
                ));
            }
            std::fs::remove_dir_all(destination).map_err(storage)?;
        }
        std::fs::rename(stage.path(), destination).map_err(storage)?;
        check(cancel)
    }
}
