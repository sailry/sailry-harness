//! File references target the matching active composer without starting a turn.
use super::{Panel, host::Host};
use crate::shell::Shell;
use gpui_kit::*;
use gpui_shell::{HostError, HostModule, HostValue};
use sailry_protocol::plugin::{Action, Reference};
use serde::Deserialize;
use std::{cell::RefCell, sync::Arc};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    path: String,
    directory: bool,
}

struct Request {
    package: Reference,
    files: Option<Vec<File>>,
    reply: RefCell<Option<tokio::sync::oneshot::Sender<bool>>>,
}
impl EventEmitter<Request> for Panel {}

pub(super) fn module(module: HostModule, owner: WeakEntity<Panel>, host: Arc<Host>) -> HostModule {
    let declarations = format!(
        "{}\nexport function canInsertFileReferences(): Promise<boolean>;\nexport function insertFileReferences(files: {{path:string;directory:boolean}}[]): Promise<boolean>;\nexport function writeClipboard(text:string): void;",
        module.declared().unwrap_or_default()
    );
    let available_owner = owner.clone();
    let available_host = host.clone();
    let clipboard = host.clone();
    module
        .async_function("canInsertFileReferences", move |_| {
            request(&available_owner, &available_host, None)
        })
        .async_function("insertFileReferences", move |args| {
            let files = serde_json::from_value(super::host::sdk::values::decode(args.value(0)?)?)
                .map_err(|error| HostError::new(error.to_string()))?;
            request(&owner, &host, Some(files))
        })
        .function("writeClipboard", move |args| {
            clipboard.check()?;
            let text = args.string(0)?.to_owned();
            gpui_shell::with_current_app(|cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(text))
            })
            .ok_or_else(|| HostError::new("clipboard requires an active view"))?;
            Ok(HostValue::Null)
        })
        .declarations(declarations)
}

fn request(
    owner: &WeakEntity<Panel>,
    host: &Host,
    files: Option<Vec<File>>,
) -> Result<impl Future<Output = Result<HostValue, HostError>> + Send + 'static + use<>, HostError>
{
    host.check()?;
    let (reply, receive) = tokio::sync::oneshot::channel();
    let event = Request {
        package: host.context().package.clone(),
        files,
        reply: RefCell::new(Some(reply)),
    };
    let owner = owner.clone();
    gpui_shell::with_current_app(|cx| {
        cx.defer(move |cx| {
            let _ = owner.update(cx, |_, cx| cx.emit(event));
        })
    })
    .ok_or_else(|| HostError::new("file references require an active view"))?;
    let stop = host.stop_token();
    Ok(async move {
        tokio::select! {
            biased;
            _ = stop.cancelled() => Err(HostError::new("plugin view is closed")),
            value = receive => value.map(HostValue::Bool).map_err(|_| HostError::new("conversation target is unavailable")),
        }
    })
}

impl Shell {
    pub(super) fn observe_plugin_references(
        panel: &Entity<Panel>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe_in(
            panel,
            window,
            |shell, panel, request: &Request, window, cx| {
                let Some(reply) = request.reply.borrow_mut().take() else {
                    return;
                };
                let panel = panel.read(cx);
                let valid = panel.resource_active()
                    && panel.selected.as_ref() == Some(&request.package)
                    && panel
                        .metadata
                        .read(cx)
                        .entries
                        .get(&request.package.name)
                        .and_then(|info| info.extension.as_ref())
                        .is_some_and(|extension| extension.actions.contains(&Action::ReadFiles));
                let target = shell
                    .current_chat()
                    .filter(|chat| {
                        let binding = chat.read(cx).binding();
                        valid
                            && binding.client.target() == panel.binding.client.target()
                            && binding.worktree.is_some()
                            && binding.worktree == panel.binding.worktree
                    })
                    .cloned();
                if let (Some(chat), Some(files)) = (&target, &request.files) {
                    let paths = files
                        .iter()
                        .map(|file| (file.path.clone(), file.directory))
                        .collect();
                    chat.update(cx, |chat, cx| {
                        chat.insert_file_references(paths, window, cx)
                    });
                }
                let _ = reply.send(target.is_some());
            },
        )
        .detach();
    }
}
