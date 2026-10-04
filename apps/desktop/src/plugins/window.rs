//! Kit exposes window activity reads but no script activation subscription.
use super::host::sdk::values::encode;
use gpui_kit::*;
use gpui_shell::{HostError, HostModule, HostValue};
use sailry_link::CancellationToken;
use serde_json::json;

#[derive(Clone)]
struct Snapshot {
    cursor: u64,
    active: bool,
}

pub(super) struct Activation {
    changes: tokio::sync::watch::Sender<Snapshot>,
}

pub(super) fn extend(
    module: HostModule,
    stop: CancellationToken,
    window: &mut Window,
    cx: &mut App,
) -> (HostModule, Entity<Activation>) {
    let activation = cx.new(|cx| {
        cx.observe_window_activation(window, |this: &mut Activation, window, _| {
            this.changes.send_modify(|state| {
                state.cursor += 1;
                state.active = window.is_window_active();
            });
        })
        .detach();
        Activation {
            changes: tokio::sync::watch::channel(Snapshot {
                cursor: 0,
                active: window.is_window_active(),
            })
            .0,
        }
    });
    let changes = activation.read(cx).changes.subscribe();
    let declarations = format!(
        "{}\nexport function nextWindowActivation(cursor?: string): Promise<{{cursor: string; active: boolean}}>;",
        module.declared().unwrap_or_default()
    );
    let module = module
        .async_function("nextWindowActivation", move |args| {
            let seen = args.get(0).and_then(HostValue::as_str).map(str::to_owned);
            let mut changes = changes.clone();
            let stop = stop.clone();
            Ok(async move {
                loop {
                    if stop.is_cancelled() {
                        return Err(HostError::new("plugin view is closed"));
                    }
                    let state = changes.borrow_and_update().clone();
                    let cursor = state.cursor.to_string();
                    if seen.as_ref() != Some(&cursor) {
                        return encode(json!({"cursor":cursor,"active":state.active}));
                    }
                    tokio::select! {
                        biased;
                        _ = stop.cancelled() => return Err(HostError::new("plugin view is closed")),
                        result = changes.changed() => result.map_err(|_| HostError::new("plugin window is closed"))?,
                    }
                }
            })
        })
        .declarations(declarations);
    (module, activation)
}
