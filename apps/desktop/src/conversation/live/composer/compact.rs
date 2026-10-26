use super::super::mode as plugin_mode;
use super::*;
use crate::conversation::{choice, mode, permission};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::scroll::ScrollableElement;

impl View {
    pub(in crate::conversation::live) fn mode_entry(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = self
            .config
            .as_ref()
            .map(|config| config.mode)
            .or(self.draft_mode)
            .unwrap_or(sailry_protocol::WorkMode::Code);
        let active = self.selected_plugin_mode(cx);
        let label = active
            .as_ref()
            .map(plugin_mode::label)
            .unwrap_or_else(|| tr(mode::label(current)));
        let icon = active
            .as_ref()
            .map(plugin_mode::icon)
            .unwrap_or_else(|| mode::icon(current).into());
        let owner = self.composer_settings.downgrade();
        Button::new("composer-mode-page")
            .ghost()
            .small()
            .h_8()
            .w_full()
            .min_w_0()
            .accessibility_label(label.clone())
            .child(row_text(
                "composer-mode",
                label,
                icon,
                Some(IconName::ChevronRight.into()),
            ))
            .debug_selector(|| "composer-mode-page".into())
            .on_click(move |_, window, cx| {
                let _ = owner.update(cx, |panel, cx| panel.show(Page::Mode, window, cx));
            })
            .into_any_element()
    }

    pub(in crate::conversation::live) fn permission_entry(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let current = self
            .config
            .as_ref()
            .map(|config| config.permission)
            .or(self.draft_permission)
            .unwrap_or(sailry_protocol::Permission::Ask);
        let owner = self.composer_settings.downgrade();
        Button::new("composer-permission-page")
            .ghost()
            .small()
            .h_8()
            .w_full()
            .min_w_0()
            .when(current == sailry_protocol::Permission::Full, |button| {
                button.text_color(cx.theme().danger)
            })
            .accessibility_label(tr(permission::label(current)))
            .child(row(
                permission::label(current),
                permission::icon(current).into(),
                Some(IconName::ChevronRight.into()),
            ))
            .debug_selector(|| "composer-permission-page".into())
            .on_click(move |_, window, cx| {
                let _ = owner.update(cx, |panel, cx| panel.show(Page::Permission, window, cx));
            })
            .into_any_element()
    }

    pub(super) fn composer_settings(&self, cx: &App) -> AnyElement {
        let panel = self.composer_settings.clone();
        let opening = panel.clone();
        let model = self.model_controls.clone();
        let current = self
            .config
            .as_ref()
            .map(|config| config.permission)
            .or(self.draft_permission)
            .unwrap_or(sailry_protocol::Permission::Ask);
        let mode = self.current_mode();
        let mode_label = self
            .selected_plugin_mode(cx)
            .map(|mode| plugin_mode::label(&mode))
            .unwrap_or_else(|| tr(mode::label(mode)));
        let hint = format!(
            "{} · {} · {}",
            tr("composer_settings"),
            mode_label,
            tr(permission::label(current))
        );
        crate::theme::popover(Popover::new("composer-settings-popover"), cx)
            .anchor(Anchor::BottomLeft)
            .bottom_2()
            .p_2()
            .trigger(
                Button::new("composer-settings")
                    .ghost()
                    .flex_shrink_0()
                    .rounded_full()
                    .icon(IconName::Settings2)
                    .tooltip(hint.clone())
                    .accessibility_label(hint)
                    .debug_selector(|| "composer-settings".into()),
            )
            .on_open_change(move |open, window, cx| {
                if *open {
                    model.update(cx, |panel, cx| panel.back(window, cx));
                    opening.update(cx, |panel, cx| panel.show(Page::Settings, window, cx));
                }
            })
            .content(move |_, _, _| panel.clone())
            .into_any_element()
    }

    fn compact_modes(&self, panel: WeakEntity<Panel>, cx: &mut Context<Self>) -> Div {
        let current = self
            .config
            .as_ref()
            .map(|config| config.mode)
            .or(self.draft_mode)
            .unwrap_or(sailry_protocol::WorkMode::Code);
        let active = self.selected_plugin_mode(cx);
        let plugins = self.plugin_modes(cx);
        v_flex()
            .gap_1()
            .children(mode::MODES.into_iter().map(|mode| {
                let panel = panel.clone();
                Button::new(mode::label(mode))
                    .ghost()
                    .h_auto()
                    .py_2()
                    .w_full()
                    .accessibility_label(format!(
                        "{}: {}",
                        tr(mode::label(mode)),
                        tr(mode::description(mode))
                    ))
                    .child(choice::content(
                        mode::label(mode).into(),
                        tr(mode::label(mode)),
                        Some(tr(mode::description(mode))),
                        mode::icon(mode).into(),
                        cx,
                    ))
                    .selected(active.is_none() && current == mode)
                    .toggled(active.is_none() && current == mode)
                    .disabled(
                        self.configuration_blocked()
                            || (self.session.is_some() && !self.connected()),
                    )
                    .debug_selector(move || format!("{}-option", mode::label(mode)))
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.select_mode(mode, window, cx);
                        let _ =
                            panel.update(cx, |panel, cx| panel.show(Page::Settings, window, cx));
                    }))
            }))
            .children(plugins.into_iter().map(|entry| {
                let panel = panel.clone();
                let selected = active.as_ref().is_some_and(|active| {
                    active.package == entry.package && active.declaration.id == entry.declaration.id
                });
                let id = SharedString::from(format!(
                    "mode-{}-{}",
                    entry.package.name, entry.declaration.id
                ));
                let label = plugin_mode::label(&entry);
                let description =
                    plugin_mode::description(&entry, self.contribution_metadata.read(cx));
                let accessible = match &description {
                    Some(description) => format!("{label}: {description}"),
                    None => label.to_string(),
                };
                Button::new(id.clone())
                    .ghost()
                    .h_auto()
                    .py_2()
                    .w_full()
                    .accessibility_label(accessible)
                    .child(choice::content(
                        id,
                        label,
                        description,
                        plugin_mode::icon(&entry),
                        cx,
                    ))
                    .selected(selected)
                    .toggled(selected)
                    .disabled(
                        self.config.is_none()
                            || self.configuration_blocked()
                            || (self.session.is_some() && !self.connected()),
                    )
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.select_plugin_mode(&entry, window, cx);
                        let _ =
                            panel.update(cx, |panel, cx| panel.show(Page::Settings, window, cx));
                    }))
            }))
    }

    fn compact_permissions(&self, panel: WeakEntity<Panel>, cx: &mut Context<Self>) -> Div {
        let current = self
            .config
            .as_ref()
            .map(|config| config.permission)
            .or(self.draft_permission)
            .unwrap_or(sailry_protocol::Permission::Ask);
        v_flex().gap_1().children(
            self.composer_options
                .permissions
                .iter()
                .copied()
                .map(|mode| {
                    let panel = panel.clone();
                    Button::new(permission::label(mode))
                        .ghost()
                        .h_auto()
                        .py_2()
                        .w_full()
                        .accessibility_label(format!(
                            "{}: {}",
                            tr(permission::label(mode)),
                            tr(permission::description(mode))
                        ))
                        .child(choice::content(
                            permission::label(mode).into(),
                            tr(permission::label(mode)),
                            Some(tr(permission::description(mode))),
                            permission::icon(mode).into(),
                            cx,
                        ))
                        .selected(current == mode)
                        .toggled(current == mode)
                        .when(mode == sailry_protocol::Permission::Full, |button| {
                            button.text_color(cx.theme().danger)
                        })
                        .disabled(
                            self.configuration_blocked()
                                || (self.session.is_some() && !self.connected()),
                        )
                        .debug_selector(move || format!("{}-option", permission::label(mode)))
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.select_permission(mode, window, cx);
                            let _ = panel
                                .update(cx, |panel, cx| panel.show(Page::Settings, window, cx));
                        }))
                }),
        )
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Settings,
    Mode,
    Permission,
}

pub(in crate::conversation::live) struct Panel {
    owner: WeakEntity<View>,
    page: Page,
    focus: FocusHandle,
}

impl Panel {
    pub(in crate::conversation::live) fn new(owner: Entity<View>, cx: &mut Context<Self>) -> Self {
        cx.observe(&owner, |_, _, cx| cx.notify()).detach();
        Self {
            owner: owner.downgrade(),
            page: Page::Settings,
            focus: cx.focus_handle(),
        }
    }

    fn show(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        self.page = page;
        self.focus.focus(window, cx);
        cx.notify();
    }
}

impl Render for Panel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = self.page;
        let owner = cx.weak_entity();
        let width = crate::model_picker::panel_size(window).width;
        let body = v_flex()
            .id("composer-settings-scroll")
            .w(width)
            .min_w_0()
            .max_h((window.viewport_size().height - px(80.)).min(px(560.)))
            .overflow_y_scrollbar()
            .gap_1()
            .track_focus(&self.focus)
            .debug_selector(|| "composer-settings-panel".into())
            .when(page != Page::Settings, |body| {
                body.child(
                    Button::new("composer-settings-back")
                        .ghost()
                        .small()
                        .h_8()
                        .w_full()
                        .min_w_0()
                        .accessibility_label(tr("composer_back"))
                        .child(row("composer_back", IconName::ChevronLeft.into(), None))
                        .debug_selector(|| "composer-settings-back".into())
                        .on_click(cx.listener(|panel, _, window, cx| {
                            panel.show(Page::Settings, window, cx)
                        })),
                )
            });
        let content = self
            .owner
            .update(cx, |view, cx| match page {
                Page::Mode => view.compact_modes(owner, cx),
                Page::Permission => view.compact_permissions(owner, cx),
                Page::Settings => v_flex()
                    .w_full()
                    .min_w_0()
                    .gap_1()
                    .child(view.model_settings(cx))
                    .children(view.registered_controls(
                        sailry_protocol::plugin::ui::Slot::Composer,
                        sailry_protocol::plugin::ui::Align::End,
                        crate::plugins::contributions::Form::Menu,
                        cx,
                    ))
                    .children(view.registered_controls(
                        sailry_protocol::plugin::ui::Slot::Composer,
                        sailry_protocol::plugin::ui::Align::Start,
                        crate::plugins::contributions::Form::Menu,
                        cx,
                    ))
                    .children(view.registered_controls(
                        sailry_protocol::plugin::ui::Slot::Context,
                        sailry_protocol::plugin::ui::Align::Start,
                        crate::plugins::contributions::Form::Menu,
                        cx,
                    ))
                    .children(view.registered_controls(
                        sailry_protocol::plugin::ui::Slot::Context,
                        sailry_protocol::plugin::ui::Align::End,
                        crate::plugins::contributions::Form::Menu,
                        cx,
                    )),
            })
            .unwrap_or_else(|_| v_flex());
        body.child(content)
    }
}

fn row(key: &'static str, icon: Icon, indicator: Option<Icon>) -> Div {
    row_text(key, tr(key), icon, indicator)
}

fn row_text(key: &'static str, label: SharedString, icon: Icon, indicator: Option<Icon>) -> Div {
    h_flex()
        .w_full()
        .min_w_0()
        .items_center()
        .gap_2()
        .child(div().size_4().flex_shrink_0().child(icon.size_4()))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_left()
                .debug_selector(move || format!("{key}-label"))
                .child(label),
        )
        .child(
            div()
                .size_4()
                .flex_shrink_0()
                .flex()
                .items_center()
                .debug_selector(move || format!("{key}-indicator"))
                .children(indicator.map(|icon| icon.size_4())),
        )
}
