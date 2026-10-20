use super::*;
use crate::conversation::usage::{self, external};
use gpui_kit::component::{progress::Progress, tag::Tag};
use sailry_protocol::{Authentication, conversation::quota, plugin::ui::Detail};
use std::rc::Rc;

mod menu;
mod summary;
#[cfg(test)]
mod tests;

pub(super) use summary::matches;
use summary::{AuthKind, Primary, Status};

const TILE: f32 = 36.;
const GAP: f32 = 12.;
/// Fixed usage column width so values right-align across rows.
const USAGE: f32 = 156.;
/// Width of the meter preceding a percentage value.
const METER: f32 = 44.;

/// The channel's type icon on its gradient tile, as in the type picker; monochrome icons
/// inherit the white text color.
pub(super) fn tile(channel: &Channel, size: f32) -> Div {
    let icon = size * 5. / 9.;
    let glyph = channel.directory.as_ref().map_or_else(
        || {
            channel
                .preset
                .category()
                .icon()
                .size(px(icon))
                .into_any_element()
        },
        |entry| presets::render_icon(entry, rems(icon / 16.)),
    );
    crate::plugins::emblem::surface(&channel.name, None, px(size))
        .text_color(gpui_kit::white())
        .child(glyph)
}

impl Workspace {
    fn provider_authentication(&self, channel: &Channel) -> Authentication {
        self.provider_link
            .as_ref()
            .and_then(|live| live.providers.get(&channel.id))
            .map_or(channel.preset.authentication(), |provider| {
                provider.authentication
            })
    }

    /// One expanded row at a time; expansion is view state only.
    fn expand_provider(&mut self, id: usize, cx: &mut Context<Self>) {
        self.providers.expanded = (self.providers.expanded != Some(id)).then_some(id);
        cx.notify();
    }

    pub(super) fn provider_row(&self, channel: &Channel, cx: &mut Context<Self>) -> AnyElement {
        let id = channel.id;
        let real = self.provider_link.is_some();
        let busy = self
            .provider_link
            .as_ref()
            .is_some_and(|live| live.pending || !live.connected);
        let authentication = self.provider_authentication(channel);
        let auth = AuthKind::of(channel, authentication);
        let expanded = self.providers.expanded == Some(id);
        let theme = cx.theme();
        let details = v_flex()
            .id(("provider-details", id))
            .debug_selector(move || format!("provider-details-{id}"))
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .gap_0p5()
            .cursor_pointer()
            .when(!channel.enabled, |text| text.opacity(0.45))
            .on_click(cx.listener(move |this, _, _, cx| this.expand_provider(id, cx)))
            .child(
                h_flex()
                    .min_w_0()
                    .gap_2()
                    .child(
                        div()
                            .debug_selector(move || format!("provider-name-{id}"))
                            .min_w_0()
                            .truncate()
                            .font_medium()
                            .child(channel.name.clone()),
                    )
                    .child(
                        Tag::secondary()
                            .xsmall()
                            .flex_shrink_0()
                            .child(summary::badge(channel, auth)),
                    )
                    .when(!real, |line| {
                        line.child(
                            Tag::secondary()
                                .xsmall()
                                .outline()
                                .flex_shrink_0()
                                .child(tr("preview")),
                        )
                    }),
            )
            .child(
                div()
                    .debug_selector(move || format!("provider-kind-{id}"))
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(channel.label()),
            );
        let summary = match Status::of(channel, authentication) {
            Some(status) => div()
                .flex_shrink_0()
                .debug_selector(move || format!("provider-status-{id}"))
                .child(
                    Tag::warning().small().child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(div().size(px(6.)).rounded_full().bg(theme.warning))
                            .child(tr(status.key())),
                    ),
                )
                .into_any_element(),
            None => Button::new(("provider-models", id))
                .ghost()
                .small()
                .flex_shrink_0()
                .debug_selector(move || format!("provider-count-{id}"))
                .label(
                    rust_i18n::t!("provider_model_count", count = channel.models.len()).to_string(),
                )
                .child(
                    Icon::new(if expanded {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .xsmall(),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.expand_provider(id, cx)))
                .into_any_element(),
        };
        let usage = Self::provider_usage(
            id,
            self.provider_metrics(channel, cx),
            channel.quota.as_ref().filter(|_| auth == AuthKind::OAuth),
            cx,
        );
        let row = h_flex()
            .py_3()
            .gap(px(GAP))
            .items_center()
            .child(
                tile(channel, TILE)
                    .when(!channel.enabled, |tile| tile.opacity(0.45))
                    .into_any_element(),
            )
            .child(details)
            .child(
                div()
                    .flex_shrink_0()
                    .when(!channel.enabled, |usage| usage.opacity(0.45))
                    .child(usage),
            )
            .child(summary)
            .child(
                Switch::new(("provider-enabled", id))
                    .disabled(busy)
                    .checked(channel.enabled)
                    .accessibility_label(tr("provider_enabled"))
                    .on_click(cx.listener(move |this, enabled, _, cx| {
                        if this.provider_link.is_some() {
                            this.provider_action(id, false, *enabled, cx);
                            return;
                        }
                        if let Some(channel) =
                            this.providers.channels.iter_mut().find(|c| c.id == id)
                        {
                            channel.enabled = *enabled;
                        }
                        cx.notify();
                    })),
            )
            .child(self.provider_menu(channel, authentication, busy, cx));
        v_flex()
            .debug_selector(move || format!("provider-{id}"))
            .w_full()
            .min_w_0()
            .child(row)
            .when(expanded, |entry| {
                entry.child(Self::provider_models(channel, cx))
            })
            .into_any_element()
    }

    /// Login quota windows as shared metric details.
    fn quota_rows(quota: &quota::Snapshot) -> Vec<Detail> {
        let now = live::quota::now();
        let windows = [
            ("provider_usage_5h", quota.five_hour.as_ref()),
            ("provider_usage_7d", quota.seven_day.as_ref()),
        ];
        windows
            .into_iter()
            .filter_map(|(key, window)| {
                let remaining = summary::remaining(window?, now)?;
                Some(Detail {
                    label: sailry_protocol::plugin::desktop::Navigation {
                        label: tr(key).to_string(),
                        icon: None,
                        locales: Default::default(),
                    },
                    value: format!("{}%", remaining.round()),
                    progress: serde_json::Number::from_f64(f64::from(remaining)),
                })
            })
            .collect()
    }

    fn provider_models(channel: &Channel, cx: &App) -> AnyElement {
        let id = channel.id;
        let (chips, hidden) =
            summary::chips(&channel.models, &channel.default_model, summary::CHIPS);
        let theme = cx.theme();
        v_flex()
            .debug_selector(move || format!("provider-models-{id}"))
            .pl(px(TILE + GAP))
            .pb_3()
            .gap_2()
            .when(!chips.is_empty(), |list| {
                list.child(
                    h_flex()
                        .flex_wrap()
                        .gap_1p5()
                        .children(chips.into_iter().map(|model| {
                            h_flex()
                                .h_6()
                                .px_2()
                                .gap_1()
                                .items_center()
                                .border_1()
                                .border_color(theme.border)
                                .rounded(theme.radius)
                                .text_xs()
                                .font_family(theme.mono_font_family.clone())
                                .when(model == channel.default_model, |chip| {
                                    chip.child(
                                        Icon::new(IconName::Star)
                                            .xsmall()
                                            .text_color(theme.warning),
                                    )
                                })
                                .child(model.to_owned())
                        }))
                        .when(hidden > 0, |list| {
                            list.child(
                                div()
                                    .h_6()
                                    .px_1()
                                    .flex()
                                    .items_center()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(format!("+{hidden}")),
                            )
                        }),
                )
            })
            .into_any_element()
    }
}

impl Workspace {
    /// The primary metric in two lines matching the name and type lines; every metric
    /// appears in the shared metric popover on hover. Plugin metrics replace login quota.
    fn provider_usage(
        id: usize,
        entries: Vec<crate::plugins::contributions::Entry>,
        quota: Option<&quota::Snapshot>,
        cx: &App,
    ) -> AnyElement {
        let column = v_flex()
            .id(("provider-usage", id))
            .debug_selector(move || format!("provider-usage-{id}"))
            .w(px(USAGE))
            .flex_shrink_0()
            .items_end()
            .gap_0p5();
        let locale = rust_i18n::locale();
        let (primary, content): (_, Rc<dyn Fn(&App) -> AnyElement>) = if let Some(entry) =
            entries.first()
        {
            let label = entry
                .state
                .label
                .as_ref()
                .unwrap_or(&entry.declaration.label);
            let primary = Primary::of(&entry.state.details, Some((label, entry.value())), &locale);
            (primary, Rc::new(move |cx| external::panel(&entries, cx)))
        } else {
            let rows = quota.map(Self::quota_rows).unwrap_or_default();
            let primary = Primary::of(&rows, None, &locale);
            let key = format!("provider-quota-{id}");
            (
                primary,
                Rc::new(move |cx| external::rows(key.clone(), &rows, cx)),
            )
        };
        let Some(primary) = primary else {
            return column.into_any_element();
        };
        let theme = cx.theme();
        let name = primary
            .label
            .clone()
            .unwrap_or_else(|| primary.value.clone());
        column
            .tooltip(move |window, cx| usage::popover(content.clone(), window, cx))
            .child(
                h_flex()
                    .h_5()
                    .gap_2()
                    .items_center()
                    .text_sm()
                    .when_some(primary.progress, |line, value| {
                        line.child(
                            Progress::new(("provider-usage-meter", id))
                                .xsmall()
                                .w(px(METER))
                                .value(value)
                                .accessibility_label(name)
                                .when(value < 20., |bar| bar.color(theme.warning)),
                        )
                    })
                    .child(
                        div()
                            .whitespace_nowrap()
                            .font_features(external::tabular())
                            .text_color(theme.foreground)
                            .child(primary.value),
                    ),
            )
            .when_some(primary.label, |column, label| {
                column.child(
                    div()
                        .max_w_full()
                        .truncate()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(label),
                )
            })
            .into_any_element()
    }
}
