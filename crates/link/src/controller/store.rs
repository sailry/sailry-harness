use super::*;
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
    sync::Mutex,
};

const MAX_PEERS: usize = 128;
const MAX_BYTES: u64 = 1024 * 1024;

#[cfg(test)]
mod tests;

#[derive(Clone, Serialize, Deserialize)]
struct Peer {
    id: NodeId,
    address: Option<EndpointAddr>,
}
#[derive(Serialize, Deserialize)]
struct Record {
    version: u32,
    peers: Vec<Peer>,
}

pub(super) struct Store(Arc<Inner>);
struct Inner {
    path: PathBuf,
    peers: Mutex<Vec<Peer>>,
    lock: Mutex<Option<File>>,
}

impl Store {
    async fn read(&self) -> Result<Vec<Peer>, Fault> {
        let inner = self.0.clone();
        tokio::task::spawn_blocking(move || Ok(inner.peers.lock().map_err(storage)?.clone()))
            .await
            .map_err(storage)?
    }
    pub(super) fn open(path: &Path) -> Result<(Identity, Self), Fault> {
        if !path.is_absolute() {
            return Err(storage("absolute controller directory required"));
        }
        #[cfg(not(unix))]
        return Err(storage(
            "controller profile protection is not implemented on this platform",
        ));
        #[cfg(unix)]
        {
            use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(path)
                .map_err(storage)?;
            let metadata = path.symlink_metadata().map_err(storage)?;
            // SAFETY: geteuid has no arguments or memory preconditions.
            if !metadata.is_dir()
                || metadata.is_symlink()
                || metadata.uid() != unsafe { libc::geteuid() }
                || metadata.permissions().mode() & 0o077 != 0
            {
                return Err(storage(
                    "controller directory must be private and owned by the current user",
                ));
            }
            let path = path.canonicalize().map_err(storage)?;
            // Share Node's lock name: one profile must never bind two endpoints,
            // even if a caller accidentally opens a Node profile as a controller.
            let lock = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(path.join("node.lock"))
                .map_err(storage)?;
            lock.try_lock().map_err(storage)?;
            if path.join("storage/node.sqlite3").exists() {
                return Err(storage("a Node profile cannot be opened as a controller"));
            }
            let identity = Identity::open(&path).map_err(storage)?;
            let peers = match OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(path.join("peers.json"))
            {
                Ok(file) => {
                    let metadata = file.metadata().map_err(storage)?;
                    if !metadata.is_file()
                        || metadata.len() > MAX_BYTES
                        || metadata.permissions().mode() & 0o077 != 0
                    {
                        return Err(storage("invalid private peer file"));
                    }
                    let mut bytes = Vec::new();
                    file.take(MAX_BYTES + 1)
                        .read_to_end(&mut bytes)
                        .map_err(storage)?;
                    let record: Record = serde_json::from_slice(&bytes).map_err(storage)?;
                    if record.version != 1 || record.peers.len() > MAX_PEERS {
                        return Err(storage("unsupported peer record"));
                    }
                    let mut unique = std::collections::BTreeSet::new();
                    for peer in &record.peers {
                        if !unique.insert(peer.id)
                            || peer
                                .address
                                .as_ref()
                                .is_some_and(|address| address.id.as_bytes() != &peer.id.0)
                        {
                            return Err(storage("invalid peer identity binding"));
                        }
                    }
                    record.peers
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => vec![],
                Err(error) => return Err(storage(error)),
            };
            if !path.join("peers.json").exists() {
                let mut file = tempfile::NamedTempFile::new_in(&path).map_err(storage)?;
                file.write_all(br#"{"version":1,"peers":[]}"#)
                    .map_err(storage)?;
                file.as_file().sync_all().map_err(storage)?;
                file.persist(path.join("peers.json")).map_err(storage)?;
                File::open(&path)
                    .and_then(|file| file.sync_all())
                    .map_err(storage)?;
            }
            Ok((
                identity,
                Self(Arc::new(Inner {
                    path,
                    peers: Mutex::new(peers),
                    lock: Mutex::new(Some(lock)),
                })),
            ))
        }
    }

    async fn update(
        &self,
        update: impl FnOnce(&mut Vec<Peer>) -> Result<(), Fault> + Send + 'static,
    ) -> Result<(), Fault> {
        let inner = self.0.clone();
        tokio::task::spawn_blocking(move || {
            let mut current = inner.peers.lock().map_err(storage)?;
            if inner.lock.lock().map_err(storage)?.is_none() {
                return Err(storage("controller store is closed"));
            }
            let mut peers = current.clone();
            update(&mut peers)?;
            if peers.len() > MAX_PEERS {
                return Err(Fault::new(
                    ErrorCode::Busy,
                    "paired device capacity reached",
                ));
            }
            let bytes = serde_json::to_vec(&Record {
                version: 1,
                peers: peers.clone(),
            })
            .map_err(storage)?;
            if bytes.len() > MAX_BYTES as usize {
                return Err(storage("peer record size exceeded"));
            }
            let mut file = tempfile::NamedTempFile::new_in(&inner.path).map_err(storage)?;
            file.write_all(&bytes).map_err(storage)?;
            file.as_file().sync_all().map_err(storage)?;
            file.persist(inner.path.join("peers.json"))
                .map_err(storage)?;
            // Rename has already committed; keep memory aligned even if fsync fails.
            *current = peers;
            File::open(&inner.path)
                .and_then(|file| file.sync_all())
                .map_err(storage)
        })
        .await
        .map_err(storage)?
    }
}

impl PeerStore for Store {
    fn close(&self) -> Pending<'_, Result<(), Fault>> {
        let inner = self.0.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                // A cancelled persistence future may still be syncing on its
                // blocking worker. Do not release the profile before it finishes.
                let _peers = inner.peers.lock().map_err(storage)?;
                inner.lock.lock().map_err(storage)?.take();
                Ok(())
            })
            .await
            .map_err(storage)?
        })
    }
    fn peers(&self) -> Pending<'_, Result<Vec<NodeId>, Fault>> {
        Box::pin(async { Ok(self.read().await?.into_iter().map(|peer| peer.id).collect()) })
    }
    fn addresses(&self) -> Pending<'_, Result<Vec<EndpointAddr>, Fault>> {
        Box::pin(async {
            Ok(self
                .read()
                .await?
                .into_iter()
                .filter_map(|peer| peer.address)
                .collect())
        })
    }
    fn remember(&self, address: EndpointAddr) -> Pending<'_, Result<(), Fault>> {
        Box::pin(self.update(move |peers| {
            let peer = peers
                .iter_mut()
                .find(|peer| &peer.id.0 == address.id.as_bytes())
                .ok_or_else(|| Fault::new(ErrorCode::PermissionDenied, "peer is not paired"))?;
            peer.address = Some(address);
            Ok(())
        }))
    }
    fn set_trust(&self, id: NodeId, trusted: bool) -> Pending<'_, Result<(), Fault>> {
        Box::pin(self.update(move |peers| {
            if trusted {
                if !peers.iter().any(|peer| peer.id == id) {
                    peers.push(Peer { id, address: None });
                }
            } else {
                peers.retain(|peer| peer.id != id);
            }
            Ok(())
        }))
    }
}
