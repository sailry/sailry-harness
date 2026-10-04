//! Local, rebuildable controller drafts; never another session or Agent history store.
use super::{Failure, Result};
use crate::{
    content::images::Local,
    conversation::live::{
        View,
        recovery::{Draft, Snapshot},
    },
};
use gpui_kit::*;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

const MAX_DATA: u64 = 16 * 1024 * 1024;

#[derive(Default)]
struct Cache {
    views: Vec<(AnyWindowHandle, WeakEntity<View>)>,
    pending: Vec<Loaded>,
    loading: bool,
    error: bool,
    reported: bool,
    expected: Arc<Mutex<Option<blake3::Hash>>>,
}
impl Global for Cache {}

#[derive(Clone)]
struct Loaded {
    marker: PathBuf,
    snapshot: Snapshot,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    version: u32,
    generation: String,
    drafts: Vec<Record>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    draft: Draft,
    attachments: Vec<Attachment>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Attachment {
    File {
        path: String,
    },
    Image {
        path: String,
        name: String,
        format: String,
    },
}

pub(super) struct Capture {
    snapshots: Vec<Snapshot>,
    expected: Arc<Mutex<Option<blake3::Hash>>>,
}

pub(crate) fn init(cx: &mut App) {
    if cx.has_global::<Cache>() {
        return;
    }
    let directory = super::service::directory(cx);
    cx.set_global(Cache {
        loading: directory.is_some(),
        ..Default::default()
    });
    let Some(directory) = directory else {
        return;
    };
    let job = cx
        .background_executor()
        .spawn(async move { load(&directory) });
    cx.spawn(async move |cx| {
        let result = job.await;
        cx.update(|cx| {
            let cache = cx.global_mut::<Cache>();
            cache.loading = false;
            match result {
                Ok((expected, pending)) => {
                    *cache
                        .expected
                        .lock()
                        .expect("draft cache revision lock poisoned") = expected;
                    cache.pending = pending;
                }
                Err(error) => {
                    eprintln!("desktop draft recovery failed: {error}");
                    cache.error = true;
                }
            }
            let views = cache.views.clone();
            for (handle, owner) in views {
                restore(handle, owner, cx);
            }
        });
    })
    .detach();
}

pub(crate) fn register(window: &Window, cx: &mut Context<View>) {
    init(cx);
    let owner = cx.weak_entity();
    let views = &mut cx.global_mut::<Cache>().views;
    views.retain(|(_, owner)| owner.upgrade().is_some());
    if !views
        .iter()
        .any(|(_, view)| view.entity_id() == owner.entity_id())
    {
        views.push((window.window_handle(), owner));
    }
    request(cx);
}

pub(crate) fn request(cx: &mut Context<View>) {
    let id = cx.entity_id();
    let selected = cx
        .try_global::<Cache>()
        .and_then(|cache| {
            cache
                .views
                .iter()
                .find(|(_, owner)| owner.entity_id() == id)
        })
        .cloned();
    if let Some((handle, owner)) = selected {
        cx.defer(move |cx| restore(handle, owner, cx));
    }
}

fn restore(handle: AnyWindowHandle, owner: WeakEntity<View>, cx: &mut App) {
    let _ = handle.update(cx, |_, window, cx| {
        let _ = owner.update(cx, |view, cx| {
            if cx.global::<Cache>().error {
                if cx.global::<Cache>().reported {
                    return;
                }
                cx.global_mut::<Cache>().reported = true;
                crate::feedback::toast(
                    window,
                    crate::tr("updates_recovery_failed"),
                    gpui_kit::component::notification::Notification::error(crate::tr(
                        "updates_recovery_failed",
                    )),
                    cx,
                );
                return;
            }
            let identity = view.recovery_identity();
            let pending = cx
                .global::<Cache>()
                .pending
                .iter()
                .find(|record| record.snapshot.draft.identity == identity)
                .cloned();
            let Some(pending) = pending else {
                return;
            };
            if !view.recover_draft(pending.snapshot, window, cx) {
                return;
            }
            cx.global_mut::<Cache>()
                .pending
                .retain(|record| record.marker != pending.marker);
            // Mark only after the UI accepted the exact scoped draft. Offline,
            // missing, busy, or already edited views keep their recovery entry.
            let marker = pending.marker;
            let job = cx.background_executor().spawn(async move {
                fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(marker)
                    .and_then(|mut file| {
                        file.write_all(b"restored")?;
                        file.sync_all()
                    })
            });
            cx.spawn_in(window, async move |_, cx| {
                if let Err(error) = job.await {
                    eprintln!("desktop draft recovery acknowledgment failed: {error}");
                    let _ = cx.update(|window, cx| {
                        crate::feedback::toast(
                            window,
                            crate::tr("updates_recovery_failed"),
                            gpui_kit::component::notification::Notification::error(crate::tr(
                                "updates_recovery_failed",
                            )),
                            cx,
                        )
                    });
                }
            })
            .detach();
        });
    });
}

pub(super) fn capture(cx: &App) -> Result<Capture> {
    crate::shell::closing::update_guard(cx)
        .map_err(|key| Failure::new(key, "open documents have unsaved changes"))?;
    if cx
        .try_global::<crate::preferences::Preferences>()
        .is_some_and(|preferences| preferences.error.is_some())
    {
        return Err(failed("desktop preferences are not readable or saved"));
    }
    let Some(cache) = cx.try_global::<Cache>() else {
        return Err(failed("desktop draft recovery is not initialized"));
    };
    if cache.loading || cache.error {
        return Err(failed("desktop draft recovery is not ready"));
    }
    let mut snapshots = Vec::new();
    for (_, owner) in &cache.views {
        if let Ok(snapshot) = owner.read_with(cx, |view, cx| view.snapshot_draft(cx)) {
            let snapshot = snapshot.map_err(|key| {
                Failure::new(key, "a composer operation or edit has not completed")
            })?;
            if let Some(snapshot) = snapshot {
                snapshots.push(snapshot);
            }
        }
    }
    snapshots.extend(cache.pending.iter().map(|record| record.snapshot.clone()));
    Ok(Capture {
        snapshots,
        expected: cache.expected.clone(),
    })
}

impl Capture {
    pub(super) fn save(self, directory: &Path) -> Result<()> {
        fs::create_dir_all(directory).map_err(Failure::io)?;
        let destination = directory.join("recovery.json");
        let current = read_optional(&destination)?;
        let mut expected = self
            .expected
            .lock()
            .map_err(|_| failed("draft cache revision is unavailable"))?;
        if current.as_deref().map(blake3::hash) != *expected {
            return Err(failed("desktop draft recovery changed before saving"));
        }
        let generation = tempfile::Builder::new()
            .prefix("drafts-")
            .tempdir_in(directory)
            .map_err(Failure::io)?;
        let name = generation
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| failed("invalid recovery generation name"))?
            .to_owned();
        let mut records = Vec::new();
        for (index, snapshot) in self.snapshots.into_iter().enumerate() {
            let record_root = generation.path().join(index.to_string());
            fs::create_dir(&record_root).map_err(Failure::io)?;
            let mut attachments = Vec::new();
            for (item, source) in snapshot.sources.into_iter().enumerate() {
                let item_root = record_root.join(item.to_string());
                fs::create_dir(&item_root).map_err(Failure::io)?;
                let source_name = source
                    .name()
                    .filter(|name| safe_name(name))
                    .ok_or_else(|| failed("an attachment has no portable file name"))?;
                let path = item_root.join(&source_name);
                let relative = format!("{index}/{item}/{source_name}");
                match source {
                    Local::Path(source) => {
                        let source = crate::resources::file_source::Source::open_limited(
                            &source,
                            &sailry_link::CancellationToken::new(),
                            sailry_protocol::attachment::MAX_BYTES,
                        )
                        .map_err(|_| failed("a draft attachment is unavailable or changed"))?;
                        let mut target = fs::OpenOptions::new()
                            .create_new(true)
                            .write(true)
                            .open(&path)
                            .map_err(Failure::io)?;
                        let copied =
                            std::io::copy(&mut source.file.take(source.size + 1), &mut target)
                                .map_err(Failure::io)?;
                        if copied != source.size {
                            return Err(failed("a draft attachment changed while saving"));
                        }
                        target.sync_all().map_err(Failure::io)?;
                        attachments.push(Attachment::File { path: relative });
                    }
                    Local::Image {
                        name,
                        format,
                        bytes,
                    } => {
                        if bytes.len() as u64 > sailry_protocol::attachment::MAX_BYTES {
                            return Err(failed("a draft image exceeds the attachment limit"));
                        }
                        let mut target = fs::OpenOptions::new()
                            .create_new(true)
                            .write(true)
                            .open(&path)
                            .map_err(Failure::io)?;
                        target.write_all(&bytes).map_err(Failure::io)?;
                        target.sync_all().map_err(Failure::io)?;
                        attachments.push(Attachment::Image {
                            path: relative,
                            name,
                            format: format.mime_type().into(),
                        });
                    }
                }
            }
            records.push(Record {
                draft: snapshot.draft,
                attachments,
            });
        }
        let data = serde_json::to_vec(&Data {
            version: 1,
            generation: name,
            drafts: records,
        })
        .map_err(|_| failed("draft recovery could not be serialized"))?;
        if data.len() as u64 > MAX_DATA {
            return Err(failed("draft recovery exceeds its metadata limit"));
        }
        let mut staged = tempfile::NamedTempFile::new_in(directory).map_err(Failure::io)?;
        staged.write_all(&data).map_err(Failure::io)?;
        staged.as_file().sync_all().map_err(Failure::io)?;
        // Keep attachment bytes before publishing their metadata. On a failed
        // metadata save, retain them for recovery instead of discarding drafts.
        let _generation = generation.keep();
        staged
            .persist(destination)
            .map_err(|error| Failure::io(error.error))?;
        *expected = Some(blake3::hash(&data));
        Ok(())
    }
}

fn load(directory: &Path) -> Result<(Option<blake3::Hash>, Vec<Loaded>)> {
    let Some(bytes) = read_optional(&directory.join("recovery.json"))? else {
        return Ok((None, vec![]));
    };
    let data: Data = serde_json::from_slice(&bytes)
        .map_err(|_| failed("draft recovery has an incompatible definition"))?;
    if data.version != 1 || !safe_name(&data.generation) {
        return Err(failed(
            "draft recovery has an incompatible version or generation",
        ));
    }
    let generation = directory
        .join(&data.generation)
        .canonicalize()
        .map_err(Failure::io)?;
    let directory = directory.canonicalize().map_err(Failure::io)?;
    if !generation.starts_with(&directory) {
        return Err(failed("draft recovery points outside the controller cache"));
    }
    let mut pending = Vec::new();
    for (index, record) in data.drafts.into_iter().enumerate() {
        let record_root = generation
            .join(index.to_string())
            .canonicalize()
            .map_err(Failure::io)?;
        if !record_root.starts_with(&generation) {
            return Err(failed("a recovery record points outside its generation"));
        }
        let marker = record_root.join("restored");
        if marker.is_file() {
            continue;
        }
        if record.attachments.len() > 8 {
            return Err(failed(
                "draft recovery exceeds the composer attachment limit",
            ));
        }
        let mut sources = Vec::new();
        for attachment in record.attachments {
            let relative = match &attachment {
                Attachment::File { path } | Attachment::Image { path, .. } => path,
            };
            if !super::manifest::safe_relative(relative) {
                return Err(failed("invalid recovered attachment path"));
            }
            let path = generation
                .join(relative)
                .canonicalize()
                .map_err(Failure::io)?;
            let metadata = fs::metadata(&path).map_err(Failure::io)?;
            if !path.starts_with(&generation)
                || !metadata.is_file()
                || metadata.len() > sailry_protocol::attachment::MAX_BYTES
            {
                return Err(failed("invalid recovered attachment file"));
            }
            sources.push(match attachment {
                Attachment::File { .. } => Local::Path(path),
                Attachment::Image { name, format, .. } => {
                    if !safe_name(&name) {
                        return Err(failed("invalid recovered image name"));
                    }
                    Local::Image {
                        name,
                        format: ImageFormat::from_mime_type(&format)
                            .ok_or_else(|| failed("unsupported recovered image format"))?,
                        bytes: Arc::from(fs::read(&path).map_err(Failure::io)?),
                    }
                }
            });
        }
        pending.push(Loaded {
            marker,
            snapshot: Snapshot {
                draft: record.draft,
                sources,
            },
        });
    }
    Ok((Some(blake3::hash(&bytes)), pending))
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(Failure::io(error)),
    };
    if file.metadata().map_err(Failure::io)?.len() > MAX_DATA {
        return Err(failed("draft recovery exceeds its metadata limit"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_DATA + 1)
        .read_to_end(&mut bytes)
        .map_err(Failure::io)?;
    if bytes.len() as u64 > MAX_DATA {
        return Err(failed("draft recovery exceeds its metadata limit"));
    }
    Ok(Some(bytes))
}

fn safe_name(name: &str) -> bool {
    super::manifest::safe_relative(name) && Path::new(name).components().count() == 1
}
fn failed(detail: &str) -> Failure {
    Failure::new("updates_drafts_failed", detail)
}

#[cfg(test)]
mod tests;
