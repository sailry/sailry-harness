use super::{Panel, metadata};
use crate::tr;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    component::{button::Button, *},
    *,
};

impl Render for Panel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.viewport.is_none() {
            crate::feedback::observe_node(window, cx, |view: &Self, cx| {
                view.source_error
                    .iter()
                    .chain(view.metadata.read(cx).diagnostics.values())
                    .find(|error| error.code == sailry_protocol::ErrorCode::Busy)
                    .cloned()
                    .map(|error| (view.binding.client.target(), error))
            });
            crate::feedback::observe_with(
                window,
                cx,
                |view: &Self, cx| {
                    let metadata = view.metadata.read(cx);
                    view.error
                        .into_iter()
                        .chain(
                            metadata
                                .errors
                                .iter()
                                .filter(|(name, _)| {
                                    !metadata.diagnostics.get(*name).is_some_and(|error| {
                                        error.code == sailry_protocol::ErrorCode::Busy
                                    })
                                })
                                .map(|(_, key)| *key),
                        )
                        .chain(
                            view.source_error
                                .as_ref()
                                .filter(|error| error.code != sailry_protocol::ErrorCode::Busy)
                                .map(|_| "live_sync_failed"),
                        )
                        .collect()
                },
                |view, key, cx| {
                    let error = if key == "live_sync_failed" {
                        view.source_error.as_ref()
                    } else {
                        view.metadata
                            .read(cx)
                            .diagnostics
                            .values()
                            .find(|error| error.code != sailry_protocol::ErrorCode::Busy)
                    };
                    error.map_or_else(
                        || gpui_kit::component::notification::Notification::error(tr(key)),
                        |error| {
                            crate::feedback::diagnostic(error.to_string().into()).title(tr(key))
                        },
                    )
                },
            );
        }
        self.sync_preview(window, cx);
        self.sync_document(window, cx);
        self.restore(window, cx);
        let viewport = window.viewport_size();
        if self.viewport != Some(viewport) {
            self.viewport = Some(viewport);
            // Kit caches script descriptions; window measurements read by a
            // plugin must be refreshed when the host viewport changes.
            if let Some(script) = self.mounted.as_ref().and_then(|mounted| {
                mounted
                    .root
                    .read(cx)
                    .content()
                    .clone()
                    .downcast::<gpui_shell::ScriptView>()
                    .ok()
            }) {
                script.update(cx, |script, cx| script.refresh(cx));
            }
        }
        let panel = v_flex()
            .id("plugin-panel")
            .debug_selector(|| "plugin-panel".into())
            .when(!self.content_sized, |panel| panel.size_full())
            .min_w_0()
            .min_h_0()
            .child(if let Some(mounted) = &self.mounted {
                div()
                    .id(("plugin-content", mounted.root.entity_id()))
                    .relative()
                    .when(!self.content_sized, |panel| panel.flex_1())
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .child(mounted.root.read(cx).content().clone())
                    .when(
                        self.initialization.loading() || !self.connected || !mounted.active(),
                        |view| {
                            view.capture_key_down(|_, _, cx| cx.stop_propagation())
                                .child(div().absolute().inset_0().occlude())
                        },
                    )
                    .into_any_element()
            } else if self.target.is_some()
                || self.selected.is_some()
                || self.initialization.loading()
            {
                let catalog = self.metadata.read(cx);
                let package = self
                    .target
                    .as_ref()
                    .and_then(|name| catalog.entries.get(name))
                    .map(|info| info.summary.reference())
                    .or_else(|| self.selected.clone());
                let metadata_failed = self
                    .target
                    .as_ref()
                    .is_some_and(|name| catalog.errors.contains_key(name));
                let unavailable = self.connected
                    && catalog.settled()
                    && package
                        .as_ref()
                        .is_none_or(|package| !self.available(package));
                let failed = !self.initialization.loading()
                    && (self.error.is_some()
                        || self.source_error.is_some()
                        || metadata_failed
                        || unavailable);
                let retryable = self.connected
                    && (metadata_failed
                        || package
                            .as_ref()
                            .is_some_and(|package| self.available(package)));
                let body = v_flex().w_full().min_h_64().child(if failed {
                    div()
                        .h_64()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            crate::feedback::retry("plugin-view-retry")
                                .debug_selector(|| "plugin-view-retry".into())
                                .disabled(!retryable)
                                .on_click(cx.listener(move |panel, _, window, cx| {
                                    if let Some(package) = &package {
                                        panel.enter(package.clone(), window, cx);
                                    } else {
                                        panel
                                            .metadata
                                            .update(cx, |metadata, cx| metadata.refresh(cx));
                                    }
                                })),
                        )
                        .into_any_element()
                } else {
                    div().w_full().h_64().into_any_element()
                });
                v_flex()
                    .id("plugin-panel-loading")
                    .debug_selector(move || {
                        if failed {
                            "plugin-panel-unavailable"
                        } else {
                            "plugin-panel-loading"
                        }
                        .into()
                    })
                    .relative()
                    .w_full()
                    .min_w_0()
                    .when(!self.content_sized, |view| {
                        view.flex_1().min_h_0().overflow_y_scroll()
                    })
                    .child(if self.content_sized {
                        body.into_any_element()
                    } else {
                        v_flex()
                            .w_full()
                            .max_w(px(crate::preview::CONTENT_WIDTH))
                            .mx_auto()
                            .p_6()
                            .gap_6()
                            .child(body)
                            .into_any_element()
                    })
                    .into_any_element()
            } else {
                v_flex()
                    .id("plugin-panel-content")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_4()
                    .gap_3()
                    .when(
                        self.selected.is_none()
                            && self.pane.is_none()
                            && self.browser_view().is_none()
                            && self.artifact.is_none()
                            && self.document.is_none()
                            && !self.is_document_page(cx),
                        |view| {
                            let catalog = self.metadata.read(cx);
                            let entries: Vec<_> = catalog
                                .entries
                                .values()
                                .filter(|info| {
                                    info.summary.enabled
                                        && info.extension.as_ref().is_some_and(|extension| {
                                            extension
                                                .desktop
                                                .as_ref()
                                                .is_some_and(|desktop| desktop.entry.is_some())
                                        })
                                })
                                .collect();
                            view.when(
                                entries.is_empty() && catalog.settled() && self.connected,
                                |view| {
                                    view.child(crate::empty_state::card(
                                        IconName::LayoutDashboard,
                                        "plugins_view_empty",
                                        cx,
                                    ))
                                },
                            )
                            .children(
                                entries.into_iter().enumerate().map(|(index, info)| {
                                    let package = info.summary.reference();
                                    Button::new(("plugin-open", index))
                                        .label(metadata::title(info))
                                        .debug_selector(move || format!("plugin-open-{index}"))
                                        .w_full()
                                        .disabled(!self.connected)
                                        .on_click(cx.listener(move |panel, _, window, cx| {
                                            panel.enter(package.clone(), window, cx)
                                        }))
                                }),
                            )
                        },
                    )
                    .into_any_element()
            });
        #[cfg(target_os = "macos")]
        let panel = panel.when_some(
            self.browser_view().filter(|_| self.resource_active()),
            |panel, browser| {
                panel.key_context("SailryBrowser").on_action(
                    move |_: &crate::browser::inspector::Inspect, _, cx| {
                        browser.update(cx, |browser, cx| browser.inspect(cx));
                    },
                )
            },
        );
        panel
    }
}

impl Panel {
    pub(crate) fn min_content_width(&self, cx: &App) -> Pixels {
        self.mounted.as_ref().map_or(px(0.), |mounted| {
            mounted.header.read(cx).min_content_width()
        })
    }

    pub(crate) fn min_content_height(&self, cx: &App) -> Pixels {
        self.mounted.as_ref().map_or(px(0.), |mounted| {
            mounted.header.read(cx).min_content_height()
        })
    }

    pub(crate) fn header(&self) -> Option<Entity<super::header::Header>> {
        self.mounted.as_ref().map(|mounted| mounted.header.clone())
    }
    pub(crate) fn heading(panel: &Entity<Self>, cx: &App) -> AnyElement {
        let panel = panel.read(cx);
        let title = panel.selected.as_ref().map_or_else(
            || tr("settings_plugins"),
            |package| {
                panel
                    .metadata
                    .read(cx)
                    .entries
                    .get(&package.name)
                    .map(metadata::title)
                    .unwrap_or_else(|| package.name.clone())
                    .into()
            },
        );
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .child(title)
            .into_any_element()
    }
}
