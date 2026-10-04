//! Kit b79f4ce's cx.bind_keys appends global bindings with no release hook.
//! Own only the registration lifetime; script on_action and GPUI focus stay native.
use crate::ui::keys::Bindings;
use gpui_kit::*;
use gpui_shell::{HostError, HostModule, HostValue, action::ShellAction};
use sailry_link::CancellationToken;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Shortcut {
    keystroke: String,
    action: String,
    context: Option<String>,
}

pub(super) fn extend(module: HostModule, stop: CancellationToken, cx: &mut App) -> HostModule {
    let bindings = Bindings::new(cx);
    let context = bindings.read(cx).context();
    let closed = bindings.downgrade();
    let cancellation = stop.clone();
    cx.spawn(async move |cx| {
        cancellation.cancelled().await;
        let _ = closed.update(cx, |bindings, cx| bindings.replace(Vec::new(), cx));
    })
    .detach();
    module.function("registerShortcuts", move |args| {
        if stop.is_cancelled() {
            return Err(HostError::new("plugin view is closed"));
        }
        let shortcuts: Vec<Shortcut> = serde_json::from_value(super::decode(
            args.get(0)
                .ok_or_else(|| HostError::new("shortcuts are required"))?,
        )?)
        .map_err(|_| HostError::new("invalid plugin shortcuts"))?;
        if shortcuts.len() > 32 {
            return Err(HostError::new("plugin shortcut limit exceeded"));
        }
        let parsed = shortcuts
            .into_iter()
            .map(|shortcut| {
                if shortcut.action.trim().is_empty()
                    || shortcut.action.len() > 128
                    || !crate::shortcuts::valid(&shortcut.keystroke)
                    || shortcut.keystroke.is_empty()
                {
                    return Err(HostError::new("invalid plugin shortcut"));
                }
                let predicate = match shortcut.context {
                    Some(relative) if !relative.trim().is_empty() && relative.len() <= 256 => {
                        format!("{context} > ({relative})")
                    }
                    Some(_) => return Err(HostError::new("invalid plugin shortcut context")),
                    None => context.clone(),
                };
                let predicate = KeyBindingContextPredicate::parse(&predicate)
                    .map_err(|_| HostError::new("invalid plugin shortcut context"))?;
                KeyBinding::load(
                    &shortcut.keystroke,
                    Box::new(ShellAction::new(shortcut.action)),
                    Some(predicate.into()),
                    false,
                    None,
                    &DummyKeyboardMapper,
                )
                .map_err(|_| HostError::new("invalid plugin shortcut"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        gpui_shell::with_current_app(|cx| {
            bindings.update(cx, |bindings, cx| bindings.replace(parsed, cx))
        })
        .ok_or_else(|| HostError::new("shortcuts require an active view"))?;
        Ok(HostValue::from(context.clone()))
    })
}

#[cfg(test)]
mod tests;
