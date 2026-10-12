use super::*;
use crate::{
    process::Environment,
    python::{Ready, Requirements},
};

fn requirements(
    directory: &Directory,
    reference: sailry_protocol::plugin::Reference,
) -> Result<Option<Requirements>, Fault> {
    if !directory.files.contains_key("requirements.txt") {
        return Ok(None);
    }
    let content = String::from_utf8(read(directory, "requirements.txt", 512 * 1024)?)
        .map_err(|_| Fault::invalid("Python requirements are not UTF-8 text"))?;
    Requirements::new(reference, content).map(Some)
}

impl Resources {
    pub(crate) async fn python(
        &self,
        key: &str,
        environment: &Environment,
    ) -> Result<Option<Ready>, Fault> {
        let (package, _) = skill(&self.packages, key)?;
        let index = self
            .packages
            .iter()
            .position(|entry| entry.summary.name == package.summary.name)
            .expect("skill belongs to its captured package");
        let required = if package.skill.is_some() {
            false
        } else {
            let directory = self.directory(index).await?;
            requirements(&directory, package.summary.reference())?.is_some()
        };
        match self.python_package(index, environment).await {
            Ok(ready) => Ok(Some(ready)),
            Err(error) if required || error.code == ErrorCode::Cancelled => Err(error),
            Err(_) => Ok(None),
        }
    }

    pub(super) async fn python_package(
        &self,
        index: usize,
        environment: &Environment,
    ) -> Result<Ready, Fault> {
        let package = &self.packages[index];
        let requirements = if package.skill.is_some() {
            None
        } else {
            let directory = self.directory(index).await?;
            requirements(&directory, package.summary.reference())?
        };
        let manager = self.host.python(environment).await?;
        if requirements.is_some()
            && manager.collection_due()
            && let Err(error) = self.host.collect_python(&manager, &self.stop).await
        {
            if error.code == ErrorCode::Cancelled {
                return Err(error);
            }
            eprintln!("Python environment references could not be refreshed: {error}");
        }
        manager.ensure(requirements.as_ref(), &self.stop).await
    }
}

impl Host {
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn register_python_environment_fixture(
        &self,
        name: &str,
        environment: &Environment,
    ) -> Result<Arc<std::sync::atomic::AtomicUsize>, Fault> {
        if self.python.get().is_some() {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "Python runtime is already initialized",
            ));
        }
        let package = self.install_bundled(name)?;
        let reference = package.summary.reference();
        let directory = self.resolve(&reference, CancellationToken::new())?;
        let requirements = requirements(&directory, reference)?
            .ok_or_else(|| Fault::invalid("fixture package requires Python dependencies"))?;
        let (manager, installations) = crate::python::Manager::ready_fixture(
            self.profile.clone().ok_or_else(unavailable)?,
            &requirements,
            Arc::new(environment.clone()),
            self.python_progress.clone(),
        )?;
        self.python.set(Arc::new(manager)).map_err(|_| {
            Fault::new(ErrorCode::Conflict, "Python runtime is already initialized")
        })?;
        Ok(installations)
    }

    pub(crate) fn retain_python(&self, packages: Vec<Info>) {
        *self
            .python_packages
            .lock()
            .expect("Python package references lock is valid") = packages;
    }

    pub(crate) async fn collect_python(
        &self,
        manager: &crate::python::Manager,
        stop: &CancellationToken,
    ) -> Result<(), Fault> {
        let packages = self
            .python_packages
            .lock()
            .expect("Python package references lock is valid")
            .clone();
        let host = self.clone();
        let cancel = stop.clone();
        let retained = tokio::task::spawn_blocking(move || {
            packages
                .into_iter()
                .filter(|package| package.skill.is_none())
                .map(|package| {
                    let reference = package.summary.reference();
                    let directory = host.resolve(&reference, cancel.clone())?;
                    requirements(&directory, reference)
                })
                .collect::<Result<Vec<_>, Fault>>()
        })
        .await
        .map_err(|_| unavailable())??;
        manager.retain(&retained.into_iter().flatten().collect::<Vec<_>>());
        Ok(())
    }

    pub(crate) async fn prepare_python(
        &self,
        info: &Info,
        environment: &Environment,
        stop: &CancellationToken,
        reporter: Option<&crate::installation::Reporter>,
    ) -> Result<Option<Ready>, Fault> {
        if info.skill.is_some() {
            return Ok(None);
        }
        let reference = info.summary.reference();
        let host = self.clone();
        let cancel = stop.clone();
        let requirements = tokio::task::spawn_blocking(move || {
            let directory = host.resolve(&reference, cancel)?;
            requirements(&directory, reference)
        })
        .await
        .map_err(|_| unavailable())??;
        let Some(requirements) = requirements else {
            return Ok(None);
        };
        if let Some(reporter) = reporter {
            reporter(sailry_protocol::installation::Phase::Preparing, None);
        }
        let manager = self.python(environment).await?;
        if let Err(error) = self.collect_python(&manager, stop).await {
            if error.code == ErrorCode::Cancelled {
                return Err(error);
            }
            eprintln!("Python environment references could not be refreshed: {error}");
        }
        manager.ensure(Some(&requirements), stop).await.map(Some)
    }
}

#[cfg(test)]
mod tests;
