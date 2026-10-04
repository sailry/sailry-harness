//! Image decoding and authenticated streams remain owned by the core gallery.
use super::{Panel, host::Host};
use crate::shell::Shell;
use gpui_kit::*;
use gpui_shell::{HostError, HostModule, HostValue};
use sailry_protocol::plugin::{Action, Context as Scope};
use std::sync::Arc;

struct Open {
    context: Scope,
    path: String,
}
impl EventEmitter<Open> for Panel {}

pub(super) fn module(module: HostModule, owner: WeakEntity<Panel>, host: Arc<Host>) -> HostModule {
    let declarations = format!(
        "{}\nexport function inspectFilePath(path:string): {{mime:string;image:boolean;external:boolean}};\nexport function openFileImage(path:string): void;",
        module.declared().unwrap_or_default()
    );
    let inspect = host.clone();
    module
        .function("inspectFilePath", move |args| {
            inspect.check()?;
            let path = args.string(0)?;
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            let external = crate::content::files::external(path);
            super::host::sdk::values::encode(serde_json::json!({
                "mime":mime.essence_str(),
                "image":crate::content::images::supports(path),
                "external":external,
            }))
        })
        .function("openFileImage", move |args| {
            host.check()?;
            let path = args.string(0)?.to_owned();
            if !crate::content::images::supports(&path) || host.context().worktree.is_none() {
                return Err(HostError::new("image requires a captured file source"));
            }
            let event = Open {
                context: host.context().clone(),
                path,
            };
            gpui_shell::with_current_app(|cx| {
                owner
                    .update(cx, |_, cx| cx.emit(event))
                    .map_err(|_| HostError::new("plugin view is closed"))
            })
            .ok_or_else(|| HostError::new("image requires an active view"))??;
            Ok(HostValue::Null)
        })
        .declarations(declarations)
}

impl Shell {
    pub(super) fn observe_plugin_images(
        panel: &Entity<Panel>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe_in(panel, window, |_, panel, event: &Open, window, cx| {
            let panel = panel.read(cx);
            if !panel.resource_active()
                || panel.selected.as_ref() != Some(&event.context.package)
                || panel.binding.worktree != event.context.worktree
                || !panel
                    .metadata
                    .read(cx)
                    .entries
                    .get(&event.context.package.name)
                    .and_then(|info| info.extension.as_ref())
                    .is_some_and(|extension| extension.actions.contains(&Action::ReadFiles))
            {
                return;
            }
            crate::content::images::open_scoped(
                panel.binding.client.clone(),
                panel.binding.runtime.clone(),
                event.context.clone(),
                event.path.clone(),
                window,
                cx,
            );
        })
        .detach();
    }
}
