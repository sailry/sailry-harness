//! Package dependency stages are shared Client snapshots, not operation outcomes.
use super::*;
use sailry_link::CancellationToken;

pub(in crate::settings) fn watch(
    binding: &crate::settings::providers::Binding,
    stop: &CancellationToken,
    cx: &mut Context<Workspace>,
) -> Task<()> {
    let node = binding.client.target();
    let client = binding.client.clone();
    let (updates, mut receiver) =
        tokio::sync::watch::channel(sailry_client::python::View::default());
    let stop = stop.clone();
    binding.runtime.spawn(async move {
        let _ = client.watch_python(updates, stop).await;
    });
    cx.spawn(async move |owner, cx| {
        loop {
            let next = receiver.borrow_and_update().clone();
            if owner
                .update(cx, |owner, cx| {
                    if owner
                        .provider_link
                        .as_ref()
                        .is_some_and(|live| live.binding.client.target() == node)
                    {
                        owner.plugin_catalog.python = next;
                        cx.notify();
                    }
                })
                .is_err()
            {
                break;
            }
            if receiver.changed().await.is_err() {
                break;
            }
        }
    })
}

pub(crate) fn label(stage: sailry_protocol::python::Stage) -> SharedString {
    tr(match stage {
        sailry_protocol::python::Stage::Creating => "python_creating",
        sailry_protocol::python::Stage::Installing => "python_installing",
        sailry_protocol::python::Stage::Validating => "python_validating",
    })
}
