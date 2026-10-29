//! Channel selection is presentation state; only the final action saves to the Node.
use super::super::presets;
use super::entry::brand_label;
use super::*;
mod methods;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    Brand,
    Method,
    Configure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Origin {
    Providers,
    Welcome,
}

pub(super) struct Flow {
    pub step: Step,
    pub brand: Option<String>,
    pub requested: Option<String>,
    editing: bool,
    origin: Origin,
}

impl Flow {
    pub fn new(editing: bool, origin: Origin) -> Self {
        Self {
            step: if editing {
                Step::Configure
            } else {
                Step::Brand
            },
            brand: None,
            requested: None,
            editing,
            origin,
        }
    }

    pub(super) fn select(&mut self, brand: &str, count: usize) {
        if self.editing {
            return;
        }
        self.requested = None;
        self.brand = Some(brand.into());
        self.step = if count == 1 {
            Step::Configure
        } else {
            Step::Method
        };
    }

    pub(super) fn defer(&mut self, brand: String) {
        if !self.editing {
            self.requested = Some(brand);
        }
    }

    fn back(&mut self, count: usize) -> Option<Origin> {
        if self.editing {
            return None;
        }
        self.step = match self.step {
            Step::Configure if count > 1 => Step::Method,
            _ => Step::Brand,
        };
        (self.step == Step::Brand).then_some(self.origin)
    }
}

/// How a method connects, shown as its icon and one-line description.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Access {
    Key,
    Endpoint,
    Login,
    Cloud,
}

impl Access {
    fn of(kind: Preset, login: bool, custom_endpoint: bool) -> Self {
        if login {
            Self::Login
        } else if kind.cloud() {
            Self::Cloud
        } else if custom_endpoint {
            Self::Endpoint
        } else {
            Self::Key
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Key => "provider_access_key",
            Self::Endpoint => "provider_access_endpoint",
            Self::Login => "provider_access_login",
            Self::Cloud => "provider_access_cloud",
        }
    }

    fn icon(self) -> Icon {
        use gpui_kit::assets::IconName as Glyph;
        match self {
            Self::Key => Icon::new(Glyph::KeyRound),
            Self::Endpoint => Icon::new(Glyph::Link),
            Self::Login => Icon::new(IconName::CircleUser),
            Self::Cloud => Icon::new(Glyph::Cloud),
        }
    }
}

/// Second summary line: the method name, or its access kind when the name
/// would only repeat the family.
fn method_summary(family: &SharedString, method: SharedString, access: Access) -> SharedString {
    if method == *family {
        tr(access.key())
    } else {
        method
    }
}

fn icon_tile(size: Pixels, icon: impl IntoElement, cx: &App) -> Div {
    div()
        .size(size)
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(cx.theme().radius)
        .bg(cx.theme().muted)
        .child(icon)
}

impl Editor {
    fn choices_count(&self) -> usize {
        self.flow
            .brand
            .as_ref()
            .and_then(|selected| {
                presets::groups(&self.directory)
                    .into_iter()
                    .find(|(key, _)| key == selected)
            })
            .map_or(0, |(_, choices)| choices.len())
    }

    pub(super) fn navigation_blocked(&self) -> bool {
        self.pending || self.closed || self.probe.is_some()
    }

    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.navigation_blocked() {
            return;
        }
        if self.flow.back(self.choices_count()) == Some(Origin::Welcome)
            && let Some(return_to) = self.return_to.clone()
        {
            self.close(window, cx);
            window.close_dialog(cx);
            return_to(window, cx);
            return;
        }
        self.scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    pub(super) fn heading(&self, cx: &Context<Self>) -> AnyElement {
        let configure = self.flow.step == Step::Configure;
        h_flex()
            .debug_selector(|| "provider-dialog-title".into())
            .w_full()
            .min_w_0()
            .when(configure, |row| row.h_6().pr_6())
            .when(!configure, |row| {
                row.pt(px(24.)).pl(px(28.)).pr(px(24.)).h(px(56.)).text_lg()
            })
            .items_center()
            .gap_2()
            .when(
                self.editing.is_none() && self.flow.step != Step::Brand,
                |row| {
                    row.child(
                        Button::new("provider-back")
                            .ghost()
                            .small()
                            .icon(IconName::ArrowLeft)
                            .accessibility_label(tr("provider_step_back"))
                            .disabled(self.navigation_blocked())
                            .on_click(cx.listener(|editor, _, window, cx| editor.back(window, cx))),
                    )
                },
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(tr(if self.editing.is_some() {
                        "provider_edit_title"
                    } else {
                        match self.flow.step {
                            Step::Brand => "provider_add_title",
                            Step::Method => "provider_choose_method",
                            Step::Configure => "provider_configure",
                        }
                    })),
            )
            .when(!configure, |row| {
                row.child(
                    div().size_8().flex_shrink_0().child(
                        gpui_kit::component::dialog::DialogClose::new()
                            .trigger(|button| button.ghost().size_8().icon(IconName::Close)),
                    ),
                )
            })
            .into_any_element()
    }

    pub(super) fn selection(&self, cx: &Context<Self>) -> impl IntoElement {
        let family = self.selection_name();
        let method = method_summary(
            &family,
            self.method_label(),
            Access::of(self.preset, self.uses_login(), self.custom_endpoint()),
        );
        h_flex()
            .w_full()
            .min_w_0()
            .items_center()
            .gap_3()
            .p_3()
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius_lg)
            .bg(cx.theme().secondary)
            .child(icon_tile(px(40.), self.directory_icon(), cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(div().truncate().font_medium().child(family))
                    .child(
                        div()
                            .truncate()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(method),
                    ),
            )
            .when(self.editing.is_none(), |row| {
                row.child(
                    Button::new("provider-change")
                        .ghost()
                        .small()
                        .label(tr("provider_change"))
                        .disabled(self.navigation_blocked())
                        .on_click(cx.listener(|editor, _, _, cx| {
                            if !editor.navigation_blocked() {
                                editor.flow.step = Step::Brand;
                                cx.notify();
                            }
                        })),
                )
            })
    }

    pub(super) fn choices(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let width = (window.viewport_size().width - px(48.)).min(super::super::brands::WIDTH);
        let content = if self.flow.step == Step::Brand {
            let owner = cx.entity().downgrade();
            div()
                .pt(px(31.))
                .pb(px(45.))
                .child(super::super::brands::matrix(
                    Some(&self.directory),
                    width,
                    self.navigation_blocked(),
                    move |brand, window, cx| {
                        let _ =
                            owner.update(cx, |editor, cx| editor.choose_brand(brand, window, cx));
                    },
                ))
                .into_any_element()
        } else {
            methods::render(self, cx)
        };
        v_flex()
            .id("provider-choices")
            .w_full()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(content)
            .vertical_scrollbar(&self.scroll)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests;
