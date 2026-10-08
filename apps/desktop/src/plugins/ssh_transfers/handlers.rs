use super::*;
use gpui_shell::{HostArguments, HostResult};

pub(super) fn upload_dropped(
    args: &HostArguments,
    drop_owner: &WeakEntity<Panel>,
    drop_host: &Arc<Host>,
    drop_store: &Entity<Store>,
    drops: &super::super::tree::drops::Drops,
) -> HostResult {
    let profile = serde_json::from_value(decode(args.value(0)?)?)
        .map_err(|error| HostError::new(error.to_string()))?;
    let directory = args.string(1)?.to_owned();
    let handle = args.string(2)?;
    let (access, _) = access(drop_owner, drop_host, profile, true, true)?;
    let paths = drops.take(handle)?;
    let ids = gpui_shell::with_current_app(|cx| {
        drop_store.update(cx, |store, _| {
            let mut ids = Vec::new();
            for source in paths {
                let name = source
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(missing)?
                    .to_owned();
                let path = worker::join(&directory, &name);
                ids.push(store.insert(access.clone(), Source::Upload(source), path));
            }
            Ok::<_, HostError>(ids)
        })
    })
    .ok_or_else(missing)??;
    encode(json!(ids))
}

pub(super) fn select_upload(
    args: &HostArguments,
    select_owner: &WeakEntity<Panel>,
    select_host: &Arc<Host>,
    select_store: &Entity<Store>,
) -> Result<impl std::future::Future<Output = HostResult> + Send + 'static + use<>, HostError> {
    let profile = serde_json::from_value(decode(args.value(0)?)?)
        .map_err(|error| HostError::new(error.to_string()))?;
    let directory = args.string(1)?.to_owned();
    let prompt = args.string(2)?.to_owned();
    let (access, _) = access(select_owner, select_host, profile, true, true)?;
    let selected = gpui_shell::with_current_app(|cx| {
        cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: Some(prompt.into()),
        })
    })
    .ok_or_else(missing)?;
    let stop = select_host.stop_token();
    let store = select_store.clone();
    let (reply, receive) = tokio::sync::oneshot::channel();
    gpui_shell::with_current_app(|cx| {
        cx.spawn(async move |cx| {
            let result = match selected.await {
                Ok(Ok(Some(paths))) if !stop.is_cancelled() => store
                    .update(cx, |store, _| {
                        let mut ids = Vec::new();
                        for source in paths {
                            let name = source
                                .file_name()
                                .and_then(|name| name.to_str())
                                .ok_or_else(missing)?
                                .to_owned();
                            let path = worker::join(&directory, &name);
                            ids.push(store.insert(access.clone(), Source::Upload(source), path));
                        }
                        Ok::<_, HostError>(ids)
                    })
                    .map_err(|_| missing()),
                Ok(Ok(None)) => Ok(Vec::new()),
                _ => Err(missing()),
            };
            let _ = reply.send(result);
        })
        .detach();
    })
    .ok_or_else(missing)?;
    Ok(async move { encode(json!(receive.await.map_err(|_| missing())??)) })
}

pub(super) fn select_download(
    args: &HostArguments,
    download_owner: &WeakEntity<Panel>,
    download_host: &Arc<Host>,
    download_store: &Entity<Store>,
) -> Result<impl std::future::Future<Output = HostResult> + Send + 'static + use<>, HostError> {
    #[derive(serde::Deserialize)]
    struct Entry {
        path: String,
        directory: bool,
    }
    let profile = serde_json::from_value(decode(args.value(0)?)?)
        .map_err(|error| HostError::new(error.to_string()))?;
    let entries: Vec<Entry> = serde_json::from_value(decode(args.value(1)?)?)
        .map_err(|error| HostError::new(error.to_string()))?;
    let prompt = args.string(2)?.to_owned();
    let (access, _) = access(download_owner, download_host, profile, false, true)?;
    if entries.is_empty() {
        return Err(missing());
    }
    let stop = download_host.stop_token();
    let store = download_store.clone();
    let (reply, receive) = tokio::sync::oneshot::channel();
    gpui_shell::with_current_app(|cx| {
        if entries.len() == 1 {
            let entry = entries.into_iter().next().unwrap();
            let selected = cx.prompt_for_new_path(
                &std::env::home_dir().unwrap_or_default(),
                entry.path.rsplit('/').next(),
            );
            cx.spawn(async move |cx| {
                let result = match selected.await {
                    Ok(Ok(Some(path))) if !stop.is_cancelled() => store
                        .update(cx, |store, _| {
                            Ok::<_, HostError>(vec![store.insert(
                                access,
                                Source::Download(path, entry.directory),
                                entry.path,
                            )])
                        })
                        .map_err(|_| missing()),
                    Ok(Ok(None)) => Ok(Vec::new()),
                    _ => Err(missing()),
                };
                let _ = reply.send(result);
            })
            .detach();
        } else {
            let selected = cx.prompt_for_paths(PathPromptOptions {
                files: false,
                directories: true,
                multiple: false,
                prompt: Some(prompt.into()),
            });
            cx.spawn(async move |cx| {
                let result = match selected.await {
                    Ok(Ok(Some(paths))) if !stop.is_cancelled() => store
                        .update(cx, |store, _| {
                            let root = paths.first().ok_or_else(missing)?;
                            let mut ids = Vec::new();
                            for entry in entries {
                                let name = entry
                                    .path
                                    .rsplit('/')
                                    .next()
                                    .filter(|name| {
                                        !name.is_empty()
                                            && !matches!(*name, "." | "..")
                                            && !name.contains('\\')
                                    })
                                    .ok_or_else(missing)?;
                                ids.push(store.insert(
                                    access.clone(),
                                    Source::Download(root.join(name), entry.directory),
                                    entry.path,
                                ));
                            }
                            Ok::<_, HostError>(ids)
                        })
                        .map_err(|_| missing()),
                    Ok(Ok(None)) => Ok(Vec::new()),
                    _ => Err(missing()),
                };
                let _ = reply.send(result);
            })
            .detach();
        }
    })
    .ok_or_else(missing)?;
    Ok(async move { encode(json!(receive.await.map_err(|_| missing())??)) })
}

pub(super) fn transfer_action(
    args: &HostArguments,
    action_owner: &WeakEntity<Panel>,
    action_host: &Arc<Host>,
    actions: &Entity<Store>,
) -> HostResult {
    action_host.check()?;
    let id = args.string(0)?.parse().map_err(HostError::new)?;
    let kind = args.string(1)?.to_owned();
    // Kit scopes reject a nested mutable App borrow. Finish the job
    // lookup before validating access, then mutate in a fresh scope.
    let (profile, node, package, write) = gpui_shell::with_current_app(|cx| {
        let state = actions.read(cx);
        let job = state.jobs.get(&id).ok_or_else(missing)?;
        Ok::<_, HostError>((
            job.access.profile.clone(),
            job.access.client.target(),
            job.access.context.package.clone(),
            matches!(job.source, Source::Upload(_)),
        ))
    })
    .ok_or_else(missing)??;
    let (access, runtime) = access(
        action_owner,
        action_host,
        profile,
        write,
        matches!(kind.as_str(), "start" | "replace"),
    )?;
    if node != access.client.target() || package != access.context.package {
        return Err(missing());
    }
    gpui_shell::with_current_app(|cx| {
        actions.update(cx, |state, cx| state.action(id, &kind, runtime, cx))
    })
    .ok_or_else(missing)??;
    Ok(HostValue::Null)
}

pub(super) fn next_transfers(
    args: &HostArguments,
    read_host: &Arc<Host>,
    store: &Entity<Store>,
) -> Result<impl std::future::Future<Output = HostResult> + Send + 'static + use<>, HostError> {
    read_host.check()?;
    let seen = args.string(0)?.to_owned();
    let mut receiver = gpui_shell::with_current_app(|cx| store.read(cx).changes.subscribe())
        .ok_or_else(missing)?;
    let package = read_host.context().package.clone();
    let node = read_host.target();
    let stop = read_host.stop_token();
    Ok(async move {
        loop {
            let values = receiver.borrow_and_update().clone();
            let values = values
                .as_array()
                .map(|values| {
                    values
                        .iter()
                        .filter(|value| {
                            value["package"] == serde_json::to_value(&package).unwrap()
                                && value["node"] == serde_json::to_value(node).unwrap()
                        })
                        .map(|value| value["value"].clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let cursor = blake3::hash(
                &serde_json::to_vec(&values).map_err(|error| HostError::new(error.to_string()))?,
            )
            .to_hex()
            .to_string();
            if cursor != seen {
                return encode(json!({"cursor":cursor,"transfers":values}));
            }
            tokio::select! {biased;_=stop.cancelled()=>return Err(missing()),result=receiver.changed()=>result.map_err(|_|missing())?}
        }
    })
}
