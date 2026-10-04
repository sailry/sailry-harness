//! Companion outputs use the same confined, non-overwriting save target as uploads.
use super::*;

pub(crate) struct Generated {
    target: save::Target,
    path: String,
}

impl Files {
    pub(crate) async fn media_input(
        &self,
        root: PathBuf,
        relative: String,
        stop: CancellationToken,
    ) -> Result<Vec<u8>, Fault> {
        let permit = self
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| Fault::new(ErrorCode::Busy, "file read capacity exhausted"))?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let control = Control {
                cancelled: stop,
                deadline: Instant::now() + Duration::from_secs(5),
            };
            let parts = path::components(&relative, false)?;
            let dir = path::descend(path::root(&root)?, &parts[..parts.len() - 1])?;
            let mut file = open_regular(&dir, parts[parts.len() - 1])?;
            let limit = adk_core::MAX_INLINE_DATA_SIZE;
            let bytes = read_bytes(&mut file, limit + 1, &control)?;
            if bytes.len() > limit {
                return Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "media input exceeds the model inline limit",
                ));
            }
            Ok(bytes)
        })
        .await
        .map_err(|_| Fault::new(ErrorCode::Internal, "media file worker failed"))?
    }

    pub(crate) async fn prepare_media(
        &self,
        root: PathBuf,
        path: String,
        limit: u64,
    ) -> Result<Generated, Fault> {
        let permit = self
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| Fault::new(ErrorCode::Busy, "file write capacity exhausted"))?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            path::entry_components(&path)?;
            path::create_parents(&root, &path)?;
            Ok(Generated {
                target: save::Target::prepare(&root, &path, None, limit, None)?,
                path,
            })
        })
        .await
        .map_err(|_| Fault::new(ErrorCode::Internal, "media file worker failed"))?
    }

    pub(crate) async fn publish_media(
        &self,
        output: Generated,
        bytes: Vec<u8>,
        stop: CancellationToken,
    ) -> Result<String, Fault> {
        let permit = self
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| Fault::new(ErrorCode::Busy, "file write capacity exhausted"))?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            if stop.is_cancelled() {
                return Err(Fault::new(ErrorCode::Cancelled, "media save cancelled"));
            }
            output.target.write(
                bytes.as_slice(),
                bytes.len() as u64,
                &blake3::hash(&bytes).to_hex(),
                #[cfg(test)]
                &|_| {},
            )?;
            Ok(output.path)
        })
        .await
        .map_err(|_| {
            Fault::new(
                ErrorCode::OutcomeUnknown,
                "media file save could not be confirmed",
            )
        })?
    }
}
