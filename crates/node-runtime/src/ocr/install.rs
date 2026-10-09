use super::*;

struct Installation {
    owner: Arc<Service>,
    id: String,
}
impl Drop for Installation {
    fn drop(&mut self) {
        let mut state = self
            .owner
            .state
            .lock()
            .expect("OCR configuration lock is valid");
        if let Some(transfer) = state.transfers.remove(&self.id) {
            transfer.cancel.cancel();
        }
        self.owner.updates.send_replace(infos(&state));
        self.owner.finished.notify_waiters();
    }
}

impl Service {
    pub(crate) async fn install(
        self: &Arc<Self>,
        id: &str,
        mirror: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<(), Fault> {
        if !sailry_protocol::ocr::valid_id(id) {
            return Err(Fault::invalid("invalid OCR model ID"));
        }
        let _mutation = self
            .mutation
            .try_lock()
            .map_err(|_| Fault::new(ErrorCode::Busy, "an OCR model is being changed"))?;
        self.models().await?;
        let (definition, stop) = {
            let mut state = self.state.lock().expect("OCR configuration lock is valid");
            if !state.transfers.is_empty() {
                return Err(Fault::new(
                    ErrorCode::Busy,
                    "an OCR model is already downloading",
                ));
            }
            let definition = state
                .definitions
                .get(id)
                .cloned()
                .ok_or_else(|| Fault::invalid("OCR model is not registered"))?;
            if state.installed.contains_key(id) {
                return Ok(());
            }
            let stop = self.stop.child_token();
            state.transfers.insert(
                id.to_owned(),
                Transfer {
                    cancel: stop.clone(),
                    progress: sailry_model_assets::Progress {
                        percent: 0.0,
                        bytes_per_second: 0.0,
                    },
                },
            );
            self.updates.send_replace(infos(&state));
            (definition, stop)
        };
        let _guard = Installation {
            owner: self.clone(),
            id: id.to_owned(),
        };
        let downloads = definition
            .assets
            .iter()
            .map(|asset| {
                Ok(sailry_model_assets::Download {
                    file: sailry_model_assets::File {
                        name: &asset.name,
                        size: asset.bytes,
                        sha256: &asset.sha256,
                    },
                    url: sailry_model_assets::url(asset, mirror).map_err(failure)?,
                })
            })
            .collect::<Result<Vec<_>, Fault>>()?;
        let directory = self.root()?.join(id);
        let operation = async {
            sailry_model_assets::prepare(&directory, &downloads, &stop, |progress| {
                let mut state = self.state.lock().expect("OCR configuration lock is valid");
                if let Some(transfer) = state.transfers.get_mut(id) {
                    transfer.progress = progress;
                    self.updates.send_replace(infos(&state));
                }
            })
            .await
            .map_err(failure)?;
            let directory = directory.clone();
            let stopping = stop.clone();
            let definition = definition.clone();
            let validated = tokio::task::spawn_blocking(move || {
                sailry_model_assets::session::background();
                if stopping.is_cancelled() {
                    return Err(cancelled());
                }
                let before = files::candidate(&directory, &definition).ok_or_else(unavailable)?;
                let _engine = sailry_ocr::Engine::load(&definition, &directory).map_err(failure)?;
                if stopping.is_cancelled() {
                    return Err(cancelled());
                }
                files::mark(&directory, &definition, &before)
            })
            .await
            .map_err(failure)??;
            if stop.is_cancelled() {
                return Err(cancelled());
            }
            self.state
                .lock()
                .expect("OCR configuration lock is valid")
                .installed
                .insert(id.to_owned(), validated);
            Ok(())
        };
        tokio::select! { biased; _ = cancel.cancelled() => { stop.cancel(); Err(cancelled()) }, _ = self.stop.cancelled() => { stop.cancel(); Err(cancelled()) }, result = operation => result }
    }

    pub(crate) async fn remove(self: &Arc<Self>, id: &str) -> Result<(), Fault> {
        if !sailry_protocol::ocr::valid_id(id)
            || !self
                .state
                .lock()
                .expect("OCR configuration lock is valid")
                .definitions
                .contains_key(id)
        {
            return Err(Fault::invalid("OCR model is not registered"));
        }
        self.cancel(id);
        let _mutation = self.mutation.lock().await;
        loop {
            let notified = self.finished.notified();
            if !self
                .state
                .lock()
                .expect("OCR configuration lock is valid")
                .transfers
                .contains_key(id)
            {
                break;
            }
            tokio::select! { _ = self.stop.cancelled() => return Err(cancelled()), _ = notified => () }
        }
        let owner = self.clone();
        let id = id.to_owned();
        tokio::task::spawn_blocking(move || {
            let mut loaded = owner.engine.lock().expect("OCR inference lock is valid");
            if loaded.as_ref().is_some_and(|entry| entry.id == id) {
                *loaded = None;
            }
            let root = owner.root()?;
            let directory = crate::files::path::root(&root)?;
            use cap_fs_ext::DirExt;
            match directory.open_dir_nofollow(&id) {
                Ok(_) => directory.remove_dir_all(&id).map_err(failure)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => return Err(failure(error)),
            }
            let mut state = owner.state.lock().expect("OCR configuration lock is valid");
            state.installed.remove(&id);
            owner.updates.send_replace(infos(&state));
            Ok(())
        })
        .await
        .map_err(failure)?
    }
}
