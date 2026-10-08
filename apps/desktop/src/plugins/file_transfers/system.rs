//! The selected Node owns resolution; remote files use a retained verified copy.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Open,
    Reveal,
}

/// Keep native dispatch at one boundary; GPUI's test platform has no file manager.
pub(crate) fn show(action: Action, path: &std::path::Path, cx: &mut App) {
    #[cfg(test)]
    {
        cx.default_global::<Observed>()
            .0
            .push((action, path.to_path_buf()));
    }
    #[cfg(not(test))]
    match action {
        Action::Open => cx.open_with_system(path),
        Action::Reveal => cx.reveal_path(path),
    }
}

#[cfg(test)]
#[derive(Default)]
struct Observed(Vec<(Action, PathBuf)>);
#[cfg(test)]
impl Global for Observed {}
#[cfg(test)]
pub(crate) fn take(cx: &mut App) -> Vec<(Action, PathBuf)> {
    std::mem::take(&mut cx.default_global::<Observed>().0)
}

pub(crate) struct Opened {
    pub path: PathBuf,
    pub copy: Option<tempfile::TempDir>,
}

pub(super) async fn prepare(
    access: Access,
    path: String,
    local: bool,
    temporary: PathBuf,
    stop: CancellationToken,
) -> Result<Opened, Fault> {
    access.verify(false).await?;
    prepare_in(
        access.client,
        access.context.worktree.unwrap(),
        Some(access.context),
        path,
        local,
        temporary,
        stop,
    )
    .await
}

pub(crate) async fn prepare_core(
    client: Arc<sailry_client::Client>,
    worktree: WorktreeId,
    path: String,
    local: bool,
    temporary: PathBuf,
    stop: CancellationToken,
) -> Result<Opened, Fault> {
    prepare_in(client, worktree, None, path, local, temporary, stop).await
}

async fn prepare_in(
    client: Arc<sailry_client::Client>,
    worktree: WorktreeId,
    context: Option<plugin::Context>,
    path: String,
    local: bool,
    temporary: PathBuf,
    stop: CancellationToken,
) -> Result<Opened, Fault> {
    valid_path(&path, true)?;
    if stop.is_cancelled() {
        return Err(cancelled());
    }
    if local {
        let Output::Snapshot(snapshot) = client.execute(client.prepare(Command::Snapshot)).await?
        else {
            return Err(internal());
        };
        if snapshot.node != client.target() {
            return Err(internal());
        }
        let root = snapshot
            .worktrees
            .iter()
            .find(|entry| entry.id == worktree)
            .ok_or_else(unavailable)?
            .path
            .clone();
        return tokio::task::spawn_blocking(move || {
            let root = PathBuf::from(root)
                .canonicalize()
                .map_err(|_| unavailable())?;
            let target = root.join(path).canonicalize().map_err(|_| unavailable())?;
            if !target.starts_with(root) {
                return Err(denied());
            }
            Ok(Opened {
                path: target,
                copy: None,
            })
        })
        .await
        .map_err(|_| internal())?;
    }
    let mut request = client.prepare(Command::DownloadFile {
        worktree,
        path: path.clone(),
    });
    if let Some(context) = context {
        request = request.with_plugin(context);
    }
    let Output::FileDownload(download) = client.execute(request).await? else {
        return Err(internal());
    };
    if download.worktree != worktree || download.path != path {
        worker::release(&client, download.stream).await;
        return Err(internal());
    }
    let temporary = temporary
        .join("open")
        .join(
            client
                .target()
                .0
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
        )
        .join(worktree.to_string());
    let prepared = tokio::task::spawn_blocking(move || {
        std::fs::create_dir_all(&temporary).map_err(|_| unavailable())?;
        let copy = tempfile::Builder::new()
            .prefix("sailry-open-")
            .tempdir_in(&temporary)
            .map_err(|_| unavailable())?;
        let name = std::path::Path::new(&path)
            .file_name()
            .ok_or_else(unavailable)?;
        let path = copy.path().join(name);
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| unavailable())?;
        Ok::<_, Fault>((
            Opened {
                path,
                copy: Some(copy),
            },
            file,
        ))
    })
    .await
    .map_err(|_| internal())?;
    let (opened, file) = match prepared {
        Ok(value) => value,
        Err(error) => {
            worker::release(&client, download.stream).await;
            return Err(error);
        }
    };
    let mut file = tokio::fs::File::from_std(file);
    client.download(&download, &mut file, stop, |_| {}).await?;
    use tokio::io::AsyncWriteExt as _;
    file.flush().await.map_err(|_| unavailable())?;
    drop(file);
    Ok(opened)
}

#[cfg(test)]
mod tests;
