//! A package composes previews using a captured file and the core native surface.
use super::{Panel, host::Host, host::sdk::values::encode};
use crate::conversation::live::{Binding, View as Chat};
use gpui_kit::*;
use gpui_shell::{HostModule, HostValue};
use sailry_protocol::tool::File;
use std::{cell::RefCell, rc::Rc, sync::Arc};

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
                info.summary.enabled
                    && info.extension.as_ref().is_some_and(|extension| {
                        extension.desktop.as_ref().is_some_and(|desktop| {
                            desktop.entry.is_some()
                                && desktop.previews.contains(&resource.file.mime)
                        })
                    })
            })
            .map(|info| info.summary.reference());
        if let Some(package) = package
            && self.selected.as_ref() != Some(&package)
        {
            self.open(package, window, cx);
        }
    }
}

pub(super) fn module(
    module: HostModule,
    resource: Option<Resource>,
    host: Arc<Host>,
    _: &mut App,
) -> HostModule {
    let declarations = format!(
        "{}\nexport function fileResource(): {{ path:string; name:string|null; mime:string; size:number|null; revision:string|null }}|null;\nexport const FilePreview: {{ new(id:string): import('gpui-kit').Element }};",
        module.declared().unwrap_or_default()
    );
    let captured = resource.as_ref().map(|resource| resource.file.clone());
    let reader = host.clone();
    let view = Rc::new(RefCell::new(None::<AnyView>));
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
                *view = crate::content::files::preview(
                    resource.binding.clone(),
                    resource.file.clone(),
                    resource.source.clone(),
                    host.context().clone(),
                    cx,
                );
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
