//! Channel selection is presentation state; only the final action saves to the Node.
use super::super::brands::{self, Choice};
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
    pub requested: Option<Choice>,
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

    pub(super) fn select(&mut self, brand: &str, methods: usize) {
        if self.editing {
            return;
        }
        self.requested = None;
        self.brand = Some(brand.into());
        self.step = if methods == 1 {
            Step::Configure
        } else {
            Step::Method
        };
    }

    pub(super) fn defer(&mut self, choice: Choice) {
        if !self.editing {
            self.requested = Some(choice);
        }
    }

    fn back(&mut self, methods: usize) -> Option<Origin> {
        if self.editing {
            return None;
        }
        self.step = match self.step {
            Step::Configure if methods > 1 => Step::Method,
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

/// Methods behind the current configuration: a login opens alone, while an API
/// method returns through its brand's list.
fn method_count(
    directory: &[sailry_protocol::conversation::presets::Entry],
    brand: Option<&str>,
    selected: Option<&sailry_protocol::conversation::presets::Entry>,
) -> usize {
    if selected.is_some_and(presets::login) {
        return 1;
    }
    brand.map_or(0, |brand| brands::api_methods(directory, brand).len())
}

impl Editor {
    pub(super) fn navigation_blocked(&self) -> bool {
        self.pending || self.closed || self.probe.is_some()
    }

    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.navigation_blocked() {
            return;
        }
        let methods = method_count(
            &self.directory,
            self.flow.brand.as_deref(),
            self.selected.as_ref(),
        );
        if self.flow.back(methods) == Some(Origin::Welcome)
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

    pub(super) fn choices(&self, cx: &Context<Self>) -> AnyElement {
        let content = if self.flow.step == Step::Brand {
            let owner = cx.entity().downgrade();
            div()
                .px_7()
                .pt(px(10.))
                .pb(px(28.))
                .child(brands::picker(
                    Some(&self.directory),
                    self.navigation_blocked(),
                    move |choice, window, cx| {
                        let _ = owner.update(cx, |editor, cx| editor.choose(choice, window, cx));
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
