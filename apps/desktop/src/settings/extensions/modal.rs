//! Captured plugin configuration reuses existing settings content inside a Kit dialog.
use super::*;
use crate::settings::{plugins::configuration, providers::Binding};
use crate::theme::DialogStyle as _;
use gpui_kit::component::dialog::{DialogClose, DialogFooter};
use gpui_kit::prelude::FluentBuilder;

#[cfg(test)]
struct Captured(WeakEntity<Workspace>);
#[cfg(test)]
impl Global for Captured {}
#[cfg(test)]
pub(in crate::settings) fn workspace(cx: &App) -> Entity<Workspace> {
    cx.global::<Captured>()
        .0
        .upgrade()
        .expect("Open configuration")
}

pub(crate) fn configurable(info: &Info) -> bool {
    info.settings
        .as_ref()
        .is_some_and(|schema| !schema.properties.is_empty())
        || info
            .extension
            .as_ref()
            .and_then(|extension| extension.settings_page.as_ref())
            .is_some_and(|page| page.entry.is_some() || page.native.is_some())
}

pub(in crate::settings) fn open(
    binding: Binding,
    info: Info,
    sources: Vec<crate::plugins::usage::Source>,
    window: &mut Window,
    cx: &mut App,
) {
    if !configurable(&info) {
        return;
    }
    let page = info
        .extension
        .as_ref()
        .and_then(|extension| extension.settings_page.as_ref());
    if page.is_none_or(|page| page.entry.is_none() && page.native.is_none()) {
        configuration::panel(binding, info.summary, window, cx);
        return;
    }
    let native = page.and_then(|page| page.native);
    let name = info.summary.name.clone();
    let title = crate::plugins::metadata::title(&info);
    let workspace = cx.new(|cx| Workspace::unbound(window, cx));
    #[cfg(test)]
    cx.set_global(Captured(workspace.downgrade()));
    workspace.update(cx, |workspace, cx| {
        workspace.bind_client(binding.clone(), cx);
        workspace.usage_sources = sources;
        if let Some(metadata) = &workspace.plugin_catalog.metadata {
            metadata.update(cx, |metadata, _| {
                metadata.entries.insert(name.clone(), info);
            });
        }
        workspace.open_plugin_settings(&name, cx);
        if native == Some(desktop::NativeSettings::Code) {
            workspace.bind_code_client(binding, window, cx);
        }
    });
    let content = cx.new(|cx| {
        cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
        Content { workspace }
    });
    window.open_dialog(cx, move |dialog, window, _| {
        let close = content.clone();
        dialog
            .form_title(title.clone())
            .w((window.viewport_size().width - px(48.)).min(px(850.)))
            .max_h(window.viewport_size().height * 0.85)
            .on_close(move |_, _, cx| {
                close
                    .read(cx)
                    .workspace
                    .clone()
                    .update(cx, |workspace, cx| workspace.deactivate(cx));
            })
            .child(content.clone())
            .footer(
                DialogFooter::new().child(DialogClose::new().trigger(|button| {
                    button
                        .label(tr("settings_cancel"))
                        .debug_selector(|| "plugin-config-close".into())
                })),
            )
    });
}

struct Content {
    workspace: Entity<Workspace>,
}

impl Render for Content {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (header, content, full_width) = self.workspace.update(cx, |workspace, cx| {
            let content = workspace.plugin_settings(window, cx);
            (
                workspace.header(window, cx),
                content,
                workspace.plugin_full_width(cx),
            )
        });
        v_flex()
            .gap_4()
            .w_full()
            .min_w_0()
            .debug_selector(|| "plugin-config-content".into())
            .children(header)
            .child(
                div()
                    .when(full_width, |view| {
                        view.h(window.viewport_size().height * 0.6)
                    })
                    .child(content),
            )
    }
}
