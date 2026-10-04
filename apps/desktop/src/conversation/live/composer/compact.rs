use super::*;
use crate::conversation::{mode, permission};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::{popover::Popover, separator::Separator};

impl View {
    pub(in crate::conversation::live) fn mode_entry(&self, _cx: &mut Context<Self>) -> AnyElement {
        let current = self
            .config
            .as_ref()
            .map(|config| config.mode)
            .or(self.draft_mode)
            .unwrap_or(sailry_protocol::WorkMode::Code);
        let owner = self.composer_settings.downgrade();
        Button::new("composer-mode-page")
            .ghost()
            .w_full()
            .accessibility_label(tr(mode::label(current)))
            .child(row(mode::label(current), IconName::ChevronRight.into()))
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
            .w_full()
            .when(current == sailry_protocol::Permission::Full, |button| {
                button.text_color(cx.theme().danger)
            })
            .accessibility_label(tr(permission::label(current)))
            .child(row(
                permission::label(current),
                IconName::ChevronRight.into(),
            ))
            .debug_selector(|| "composer-permission-page".into())
            .on_click(move |_, window, cx| {
                let _ = owner.update(cx, |panel, cx| panel.show(Page::Permission, window, cx));
            })
            .into_any_element()
    }

    pub(super) fn composer_settings(&self) -> AnyElement {
        let panel = self.composer_settings.clone();
        let opening = panel.clone();
        let model = self.model_controls.clone();
        Popover::new("composer-settings-popover")
            .anchor(Anchor::BottomLeft)
            .bottom_2()
            .p_2()
            .trigger(
                Button::new("composer-settings")
                    .ghost()
                    .rounded_full()
                    .icon(IconName::Settings2)
                    .tooltip(tr("composer_settings"))
                    .accessibility_label(tr("composer_settings"))
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
                    .child(option(
                        mode::label(mode),
                        mode::description(mode),
                        mode::icon(mode),
                        current == mode,
                        cx,
                    ))
                    .selected(current == mode)
                    .toggled(current == mode)
                    .disabled(self.busy() || (self.session.is_some() && !self.connected()))
                    .debug_selector(move || format!("{}-option", mode::label(mode)))
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.select_mode(mode, window, cx);
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
                        .child(option(
                            permission::label(mode),
                            permission::description(mode),
                            permission::icon(mode),
                            current == mode,
                            cx,
                        ))
                        .selected(current == mode)
                        .toggled(current == mode)
                        .when(mode == sailry_protocol::Permission::Full, |button| {
                            button.text_color(cx.theme().danger)
                        })
                        .disabled(self.busy() || (self.session.is_some() && !self.connected()))
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
            .max_h((window.viewport_size().height - px(80.)).min(px(560.)))
            .overflow_y_scrollbar()
            .gap_1()
            .track_focus(&self.focus)
            .debug_selector(|| "composer-settings-panel".into())
            .when(page != Page::Settings, |body| {
                body.child(
                    Button::new("composer-settings-back")
                        .ghost()
                        .w_full()
                        .accessibility_label(tr("composer_back"))
                        .child(row("composer_back", IconName::ChevronLeft.into()))
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
                    .gap_1()
                    .children(view.registered_controls(
                        sailry_protocol::plugin::ui::Slot::Composer,
                        sailry_protocol::plugin::ui::Align::End,
                        crate::plugins::contributions::Form::Menu,
                        cx,
                    ))
                    .child(
                        div()
                            .py_2()
                            .debug_selector(|| "composer-settings-separator".into())
                            .child(Separator::horizontal()),
                    )
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

fn row(key: &'static str, icon: Icon) -> Div {
    h_flex()
        .w_full()
        .min_w_0()
        .gap_2()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_left()
                .debug_selector(move || format!("{key}-label"))
                .child(tr(key)),
        )
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .debug_selector(move || format!("{key}-indicator"))
                .child(icon.small()),
        )
}

fn option(
    key: &'static str,
    description: &'static str,
    icon: IconName,
    selected: bool,
    cx: &App,
) -> Div {
    h_flex()
        .w_full()
        .min_w_0()
        .gap_2()
        .child(
            div()
                .flex_shrink_0()
                .debug_selector(move || format!("{key}-icon"))
                .child(Icon::new(icon).size_4()),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .text_left()
                .whitespace_normal()
                .child(
                    div()
                        .text_sm()
                        .line_height(relative(1.25))
                        .debug_selector(move || format!("{key}-label"))
                        .child(tr(key)),
                )
                .child(
                    div()
                        .text_xs()
                        .line_height(relative(1.25))
                        .text_color(cx.theme().muted_foreground)
                        .debug_selector(move || format!("{key}-description"))
                        .child(tr(description)),
                ),
        )
        .child(
            div()
                .flex_shrink_0()
                .debug_selector(move || format!("{key}-indicator"))
                .child(
                    Icon::new(IconName::Check)
                        .small()
                        .when(!selected, |icon| icon.opacity(0.)),
                ),
        )
}
