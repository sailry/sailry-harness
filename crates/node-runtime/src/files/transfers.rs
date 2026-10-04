//! Caller-owned, short-lived file streams; the existing Link supplies authentication.
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use sailry_link::{CancellationToken, Stream};
use sailry_protocol::{
    ErrorCode, FILE_TRANSFER_CHUNK_BYTES, Fault, FileDownload, NodeId, StreamId, WorktreeId,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

mod download;
mod registry;
#[cfg(test)]
mod tests;
mod upload;

const CAPACITY: usize = 8;
const PER_CALLER: usize = 4;
const PREPARE_TIMEOUT: Duration = Duration::from_secs(8);
const OPEN_TIMEOUT: Duration = Duration::from_secs(60);
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) struct Transfers {
    entries: Mutex<Registry>,
    capacity: Arc<Semaphore>,
    profile: Option<PathBuf>,
}

#[derive(Default)]
struct Registry {
    closed: bool,
    entries: BTreeMap<StreamId, Entry>,
}

struct Entry {
    caller: NodeId,
    root: PathBuf,
    cancel: CancellationToken,
    state: State,
    expires: Instant,
}

enum State {
    Preparing,
    Ready(Resource),
    Active,
    Uploaded(Box<upload::Prepared>),
    Finishing,
    Inspecting,
}

enum Resource {
    Download(Prepared),
    Upload(Box<upload::Prepared>),
}

struct Prepared {
    source: Source,
    size: u64,
    revision: String,
    stamp: String,
    _permit: OwnedSemaphorePermit,
}

enum Source {
    File(std::fs::File),
    Bytes(Vec<u8>),
}

struct Lease {
    transfers: Arc<Transfers>,
    stream: StreamId,
    armed: bool,
}

impl Drop for Lease {
    fn drop(&mut self) {
        if self.armed {
            self.transfers.finish(self.stream);
        }
    }
}

impl Transfers {
    pub(crate) async fn office(
        self: &Arc<Self>,
        caller: NodeId,
        worktree: WorktreeId,
        root: PathBuf,
        path: String,
        closed: CancellationToken,
    ) -> Result<sailry_protocol::office::Preview, Fault> {
        let (mut lease, permit, cancel) = self.reserve(caller, root.clone(), closed)?;
        let source = path.clone();
        let stopping = cancel.clone();
        let profile = self.profile.clone();
        let worker = tokio::task::spawn_blocking(move || {
            // Conversion retains a transfer permit until CPU work has really ended.
            let source_path = root
                .join(&source)
                .canonicalize()
                .map_err(crate::files::io_error)?;
            if profile
                .as_ref()
                .is_some_and(|profile| source_path.starts_with(profile))
            {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "Node storage is not a previewable file",
                ));
            }
            let (bytes, revision, warnings) = crate::office::preview(&root, &source)?;
            if stopping.is_cancelled() {
                return Err(cancelled());
            }
            let prepared = Prepared {
                size: bytes.len() as u64,
                revision: blake3::hash(&bytes).to_hex().to_string(),
                stamp: String::new(),
                source: Source::Bytes(bytes),
                _permit: permit,
            };
            Ok::<_, Fault>((prepared, revision, warnings))
        });
        let (prepared, source_revision, warnings) = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(cancelled()),
            result = tokio::time::timeout(Duration::from_secs(120), worker) => result
                .map_err(|_| Fault::new(ErrorCode::Busy, "Office conversion timed out"))?
                .map_err(|_| unavailable())??,
        };
        let output = sailry_protocol::office::Preview {
            download: FileDownload {
                stream: lease.stream,
                worktree,
                path,
                size: prepared.size,
                revision: prepared.revision.clone(),
                stamp: String::new(),
            },
            source_revision,
            warnings,
        };
        self.ready(&mut lease, Resource::Download(prepared), cancel)?;
        Ok(output)
    }

    pub(crate) fn new(profile: Option<PathBuf>) -> Arc<Self> {
        Arc::new(Self {
            entries: Mutex::new(Registry::default()),
            capacity: Arc::new(Semaphore::new(CAPACITY)),
            profile,
        })
    }

    pub(crate) async fn download(
        self: &Arc<Self>,
        caller: NodeId,
        worktree: WorktreeId,
        root: PathBuf,
        path: String,
        closed: CancellationToken,
    ) -> Result<FileDownload, Fault> {
        let (mut lease, permit, cancel) = self.reserve(caller, root.clone(), closed)?;
        let stream = lease.stream;
        let source = path.clone();
        let profile = self.profile.clone();
        let stopping = cancel.clone();
        // A cancelled blocking read keeps its permit until the worker really exits.
        let worker = tokio::task::spawn_blocking(move || {
            download::prepare(&root, &source, profile.as_deref(), stopping, permit)
        });
        let prepared = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(cancelled()),
            result = tokio::time::timeout(PREPARE_TIMEOUT, worker) => result
                .map_err(|_| Fault::new(ErrorCode::Busy, "download preparation timed out"))?
                .map_err(|_| unavailable())??,
        };
        let output = FileDownload {
            stream,
            worktree,
            path,
            size: prepared.size,
            revision: prepared.revision.clone(),
            stamp: prepared.stamp.clone(),
        };
        self.ready(&mut lease, Resource::Download(prepared), cancel)?;
        Ok(output)
    }

    pub(crate) async fn download_attachment(
        self: &Arc<Self>,
        caller: NodeId,
        root: PathBuf,
        attachment: sailry_protocol::attachment::Attachment,
        closed: CancellationToken,
    ) -> Result<sailry_protocol::attachment::Download, Fault> {
        let (mut lease, permit, cancel) = self.reserve(caller, root, closed)?;
        let profile = self.profile.clone();
        let source = attachment.clone();
        let worker = tokio::task::spawn_blocking(move || {
            let file = crate::files::attachments::open(profile.as_deref(), &source)?;
            Ok::<_, Fault>(Prepared {
                source: Source::File(file),
                size: source.spec.size,
                revision: source.spec.revision,
                stamp: String::new(),
                _permit: permit,
            })
        });
        let prepared = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(cancelled()),
            result = tokio::time::timeout(PREPARE_TIMEOUT, worker) => result
                .map_err(|_| Fault::new(ErrorCode::Busy, "attachment preparation timed out"))?
                .map_err(|_| unavailable())??,
        };
        let output = sailry_protocol::attachment::Download {
            stream: lease.stream,
            attachment,
        };
        self.ready(&mut lease, Resource::Download(prepared), cancel)?;
        Ok(output)
    }

    pub(crate) fn download_bytes(
        self: &Arc<Self>,
        caller: NodeId,
        root: PathBuf,
        attachment: sailry_protocol::attachment::Attachment,
        bytes: Vec<u8>,
        closed: CancellationToken,
    ) -> Result<sailry_protocol::attachment::Download, Fault> {
        let (mut lease, permit, cancel) = self.reserve(caller, root, closed)?;
        if bytes.len() as u64 != attachment.spec.size
            || attachment.spec.size > sailry_protocol::attachment::MAX_BYTES
        {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "tool image size mismatch",
            ));
        }
        let prepared = Prepared {
            source: Source::Bytes(bytes),
            size: attachment.spec.size,
            revision: attachment.spec.revision.clone(),
            stamp: String::new(),
            _permit: permit,
        };
        let output = sailry_protocol::attachment::Download {
            stream: lease.stream,
            attachment,
        };
        self.ready(&mut lease, Resource::Download(prepared), cancel)?;
        Ok(output)
    }

    pub(crate) fn open(
        self: &Arc<Self>,
        caller: NodeId,
        stream: StreamId,
    ) -> Result<Stream, Fault> {
        let (resource, cancel) = {
            let mut registry = self.entries.lock().unwrap();
            let entry = registry
                .entries
                .get_mut(&stream)
                .filter(|entry| entry.caller == caller)
                .ok_or_else(missing)?;
            if matches!(entry.state, State::Ready(_)) && entry.expires <= Instant::now() {
                entry.cancel.cancel();
                registry.entries.remove(&stream);
                return Err(missing());
            }
            if !matches!(entry.state, State::Ready(_)) {
                return Err(Fault::new(
                    ErrorCode::Conflict,
                    "file stream is not ready or already opened",
                ));
            }
            let State::Ready(prepared) = std::mem::replace(&mut entry.state, State::Active) else {
                unreachable!()
            };
            (prepared, entry.cancel.clone())
        };
        let (client, mut service) = tokio::io::duplex(FILE_TRANSFER_CHUNK_BYTES);
        let mut lease = Lease {
            transfers: self.clone(),
            stream,
            armed: true,
        };
        tokio::spawn(async move {
            match resource {
                Resource::Download(prepared) => {
                    let _lease = lease;
                    tokio::select! {
                        biased;
                        _ = cancel.cancelled() => {},
                        _ = download::send(prepared, &mut service) => {},
                    }
                }
                Resource::Upload(prepared) => {
                    let received = tokio::select! {
                        biased;
                        _ = cancel.cancelled() => return,
                        result = upload::receive(prepared, &mut service) => result,
                    };
                    if let Ok(prepared) = received
                        && lease.transfers.uploaded(stream, prepared, cancel).is_ok()
                    {
                        lease.armed = false;
                        let _ = upload::acknowledge(&mut service).await;
                    }
                    drop(lease);
                }
            }
        });
        Ok(Box::new(client))
    }
}

fn busy() -> Fault {
    Fault::new(ErrorCode::Busy, "file transfer capacity exhausted")
}
fn missing() -> Fault {
    Fault::new(ErrorCode::NotFound, "file transfer is unavailable")
}
fn unavailable() -> Fault {
    Fault::new(
        ErrorCode::Unavailable,
        "file transfer service is unavailable",
    )
}
fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "file transfer cancelled")
}
