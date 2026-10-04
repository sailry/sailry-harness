//! Verified temporary copies for an admitted command, released with its process group.
use super::*;
use sailry_link::CancellationToken;
use std::{
    io::{Read, Write},
    path::PathBuf,
    time::{Duration, Instant},
};

pub(crate) struct Inputs {
    pub profile: PathBuf,
    pub files: Vec<attachment::Attachment>,
}

impl Inputs {
    pub(crate) async fn stage(self, stop: CancellationToken) -> Result<tempfile::TempDir, Fault> {
        let cancelled = stop.child_token();
        let _guard = cancelled.clone().drop_guard();
        // The command pool already holds its capacity permit; no independent task queue.
        tokio::task::spawn_blocking(move || {
            let control = super::super::Control {
                cancelled,
                deadline: Instant::now() + Duration::from_secs(30),
            };
            control.check()?;
            let directory = tempfile::Builder::new()
                .prefix("sailry-inputs-")
                .tempdir_in(&self.profile)
                .map_err(io_error)?;
            let mut buffer = [0; 64 * 1024];
            for attachment in self.files {
                control.check()?;
                validate(&attachment.spec)?;
                let mut source = open(Some(&self.profile), &attachment)?;
                let mut target =
                    std::fs::File::create_new(directory.path().join(attachment.id.to_string()))
                        .map_err(io_error)?;
                let mut hash = blake3::Hasher::new();
                let mut size = 0_u64;
                loop {
                    control.check()?;
                    let count = source.read(&mut buffer).map_err(io_error)?;
                    if count == 0 {
                        break;
                    }
                    size += count as u64;
                    if size > attachment.spec.size {
                        return Err(changed());
                    }
                    hash.update(&buffer[..count]);
                    target.write_all(&buffer[..count]).map_err(io_error)?;
                }
                if size != attachment.spec.size
                    || hash.finalize().to_hex().as_str() != attachment.spec.revision
                {
                    return Err(changed());
                }
            }
            control.check()?;
            Ok(directory)
        })
        .await
        .map_err(|_| Fault::new(ErrorCode::Internal, "attachment staging worker failed"))?
    }
}

fn changed() -> Fault {
    Fault::new(ErrorCode::RevisionConflict, "attachment content changed")
}
