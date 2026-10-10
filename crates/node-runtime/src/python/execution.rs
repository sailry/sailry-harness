use super::{Access, Manager, Ready, storage};
use cap_fs_ext::DirExt;
use sailry_link::CancellationToken;
use sailry_protocol::{Fault, SessionId};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
};

pub(crate) struct Execution {
    pub base: Ready,
    pub skills: Vec<Access>,
    pub session: PathBuf,
    pub cache: PathBuf,
    pub index: Option<String>,
}

impl Manager {
    pub(crate) async fn execution(
        &self,
        profile: &Path,
        session: SessionId,
        skills: &[Access],
        cancel: &CancellationToken,
    ) -> Result<Execution, Fault> {
        let base = self.ensure(None, cancel).await?;
        let mut refreshed = Vec::with_capacity(skills.len());
        for skill in skills {
            refreshed.push(Access {
                ready: self.refresh(&skill.ready, cancel).await?,
                directory: skill.directory.clone(),
            });
        }
        let session = directory(profile, session)?;
        Ok(Execution {
            base,
            skills: refreshed,
            session,
            cache: self.root.join("cache"),
            index: self
                .index
                .lock()
                .expect("Python index lock is valid")
                .clone(),
        })
    }
}

impl Execution {
    pub(crate) fn variables(&self) -> BTreeMap<&'static str, OsString> {
        let mut variables = BTreeMap::from([
            ("SAILRY_PYTHON", self.base.python.clone().into_os_string()),
            ("SAILRY_SESSION_DIR", self.session.clone().into_os_string()),
            ("PIP_CACHE_DIR", self.cache.clone().into_os_string()),
            ("PYTHONDONTWRITEBYTECODE", "1".into()),
        ]);
        if let Some(index) = &self.index {
            variables.insert("PIP_INDEX_URL", index.into());
        }
        variables
    }
}

fn directory(profile: &Path, session: SessionId) -> Result<PathBuf, Fault> {
    let mut anchor = crate::files::path::root(profile)?;
    for name in [
        "workspaces".to_owned(),
        "sessions".to_owned(),
        session.to_string(),
    ] {
        let mut options = cap_std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use cap_std::fs::DirBuilderExt;
            options.mode(0o700);
        }
        match anchor.create_dir_with(&name, &options) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(storage(error)),
        }
        anchor = anchor.open_dir_nofollow(name).map_err(storage)?;
    }
    Ok(crate::workspaces::session(profile, session))
}
