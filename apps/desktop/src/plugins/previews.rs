//! A package composes previews using a captured file and the core native surface.
use super::{Panel, host::Host, host::sdk::values::encode};
use crate::conversation::live::{Binding, View as Chat};
use gpui_kit::*;
use gpui_shell::{HostModule, HostValue};
use sailry_protocol::tool::File;
use std::{cell::RefCell, rc::Rc, sync::Arc};
mod notebook;

pub(super) type View = Rc<RefCell<Option<AnyView>>>;

#[derive(Clone)]
pub(crate) struct Resource {
    pub binding: Binding,
    pub file: File,
    pub source: WeakEntity<Chat>,
}

impl Panel {
    pub(crate) fn artifact(
        binding: Binding,
        file: File,
        source: WeakEntity<Chat>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut panel = Self::standalone(binding.clone(), cx);
        panel.artifact = Some(Resource {
            binding,
            file,
            source,
        });
        panel
    }

    pub(crate) fn sync_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(resource) = &self.artifact else {
            return;
        };
        if !self.connected || self.loading || !self.metadata.read(cx).settled() {
            return;
        }
        let package = self
            .metadata
            .read(cx)
            .entries
            .values()
            .find(|info| {
                accepts(
                    info,
                    if crate::content::files::notebook(&resource.file.path) {
                        "application/x-ipynb+json"
                    } else {
                        &resource.file.mime
                    },
                )
            })
            .map(|info| info.summary.reference());
        if let Some(package) = package
            && self.selected.as_ref() != Some(&package)
        {
            self.open(package, window, cx);
        }
    }

    pub(crate) fn file_edit_context(&self, cx: &App) -> Option<(u32, String)> {
        let mounted = self.mounted.as_ref().filter(|mounted| mounted.active())?;
        let view = mounted.preview.borrow();
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            crate::content::files::html::edit_context(view.as_ref()?, cx)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (view, cx);
            None
        }
    }

    #[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
    pub(crate) fn file_preview(&self) -> Option<AnyView> {
        self.mounted
            .as_ref()
            .filter(|mounted| mounted.active())?
            .preview
            .borrow()
            .clone()
    }
}

pub(super) fn accepts(info: &sailry_protocol::plugin::Info, mime: &str) -> bool {
    info.summary.enabled
        && info.extension.as_ref().is_some_and(|extension| {
            extension.desktop.as_ref().is_some_and(|desktop| {
                desktop.entry.is_some() && desktop.previews.iter().any(|value| value == mime)
            })
        })
}

pub(super) fn module(
    module: HostModule,
    resource: Option<Resource>,
    host: Arc<Host>,
    view: View,
    _: &mut App,
) -> HostModule {
    let declarations = format!(
        "{}\nexport function fileResource(): {{ path:string; name:string|null; mime:string; size:number|null; revision:string|null }}|null;\nexport const FilePreview: {{ new(id:string): import('gpui-kit').Element }};",
        module.declared().unwrap_or_default()
    );
    let captured = resource.as_ref().map(|resource| resource.file.clone());
    let reader = host.clone();
    module
        .function("fileResource", move |_| {
            reader.check()?;
            captured.as_ref().map_or(Ok(HostValue::Null), |file| {
                encode(serde_json::to_value(file).expect("file metadata is JSON"))
            })
        })
        .component("FilePreview", move |_, _, cx| {
            if host.check().is_err() {
                return div().into_any_element();
            }
            let Some(resource) = &resource else {
                return div().into_any_element();
            };
            let mut view = view.borrow_mut();
            if view.is_none() {
                *view = if crate::content::files::notebook(&resource.file.path) {
                    Some(
                        cx.new(|_| {
                            notebook::View::new(
                                resource.binding.clone(),
                                resource.file.clone(),
                                host.clone(),
                            )
                        })
                        .into(),
                    )
                } else {
                    crate::content::files::preview(
                        resource.binding.clone(),
                        resource.file.clone(),
                        resource.source.clone(),
                        host.context().clone(),
                        cx,
                    )
                };
            }
            view.as_ref().map_or_else(
                || {
                    div()
                        .p_6()
                        .child(crate::tr("artifact_preview_unavailable"))
                        .into_any_element()
                },
                |view| div().size_full().child(view.clone()).into_any_element(),
            )
        })
        .declarations(declarations)
}
