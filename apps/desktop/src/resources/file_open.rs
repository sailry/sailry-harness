//! Explicit resource actions reuse the core system-opening capability.
use crate::{backend::Services, shell::Shell, tr};
use gpui_kit::*;
use sailry_client::Client;
use sailry_link::CancellationToken;
use sailry_protocol::{NodeId, WorktreeId};
use std::{path::PathBuf, sync::Arc};

#[derive(Default)]
struct Copies(Vec<tempfile::TempDir>);
impl Global for Copies {}

impl Shell {
    pub(crate) fn open_file_in_system(
        &mut self,
        scope: (NodeId, WorktreeId),
        path: String,
        directory: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(transport) = self
            .live
            .as_ref()
            .and_then(|live| live.transport_for(scope.0))
        else {
            failure("files_open_failed", window, cx);
            return;
        };
        let services = cx.global::<Services>().clone();
        let local = services.local.target() == scope.0;
        if directory && !local {
            failure("files_remote_directory", window, cx);
            return;
        }
        let job = services
            .runtime
            .spawn(crate::plugins::file_transfers::system::prepare_core(
                Arc::new(Client::new(transport)),
                scope.1,
                path,
                local,
                CancellationToken::new(),
            ));
        window
            .spawn(cx, async move |cx| {
                let result = job
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .ok_or("files_open_failed");
                let _ = cx.update(|window, cx| match result {
                    Ok(opened) => {
                        if let Some(copy) = opened.copy {
                            if !cx.has_global::<Copies>() {
                                cx.set_global(Copies::default());
                            }
                            cx.global_mut::<Copies>().0.push(copy);
                        }
                        cx.open_with_system(&opened.path);
                    }
                    Err(key) => failure(key, window, cx),
                });
            })
            .detach();
    }
}

pub(crate) fn local(path: PathBuf, window: &mut Window, cx: &mut App) {
    let runtime = cx.global::<Services>().runtime.clone();
    let job = runtime.spawn_blocking(move || path.canonicalize());
    window
        .spawn(cx, async move |cx| {
            let result = job.await;
            let _ = cx.update(|window, cx| match result {
                Ok(Ok(path)) => cx.open_with_system(&path),
                _ => failure("files_open_failed", window, cx),
            });
        })
        .detach();
}

pub(crate) fn failure(key: &'static str, window: &mut Window, cx: &mut App) {
    crate::feedback::error(&tr("files_open"), &tr(key), window, cx);
}
