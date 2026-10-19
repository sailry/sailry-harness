use super::*;
use gpui_kit::component::{progress::Progress, tag::Tag};
use sailry_protocol::Authentication;

mod menu;
mod summary;
#[cfg(test)]
mod tests;

pub(super) use summary::matches;
use summary::{AuthKind, Status};

const TILE: f32 = 36.;
const GAP: f32 = 12.;

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
                            .child(tr(auth.key())),
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
        let row = h_flex()
            .id(("provider-row", id))
            .mx(px(-16.))
            .px_4()
            .py_3()
            .gap(px(GAP))
            .items_center()
            .hover(|style| style.bg(cx.theme().list_hover))
            .child(Self::provider_tile(channel))
            .child(details)
            .when_some(
                channel.quota.as_ref().filter(|_| auth == AuthKind::OAuth),
                |row, quota| row.child(Self::provider_quota(id, quota, cx)),
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

    fn provider_tile(channel: &Channel) -> AnyElement {
        let tile = crate::plugins::emblem::surface(&channel.name, None, px(TILE))
            .when(!channel.enabled, |tile| tile.opacity(0.45));
        // The type icon, as in the type picker; monochrome icons inherit the white text color.
        let glyph = channel.directory.as_ref().map_or_else(
            || {
                channel
                    .preset
                    .category()
                    .icon()
                    .size(px(20.))
                    .into_any_element()
            },
            |entry| presets::render_icon(entry, rems(1.25)),
        );
        tile.text_color(gpui_kit::white())
            .child(glyph)
            .into_any_element()
    }

    fn provider_quota(
        id: usize,
        quota: &sailry_protocol::conversation::quota::Snapshot,
        cx: &App,
    ) -> AnyElement {
        let now = live::quota::now();
        let windows = [
            ("provider_usage_5h", quota.five_hour.as_ref()),
            ("provider_usage_7d", quota.seven_day.as_ref()),
        ];
        v_flex()
            .debug_selector(move || format!("provider-quota-{id}"))
            .w(px(120.))
            .flex_shrink_0()
            .gap_1()
            .children(
                windows
                    .into_iter()
                    .enumerate()
                    .filter_map(|(index, (key, window))| {
                        let used = summary::usage(window?, now);
                        let label = tr(key);
                        Some(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(
                                    div()
                                        .w(px(34.))
                                        .flex_shrink_0()
                                        .truncate()
                                        .child(label.clone()),
                                )
                                .child(
                                    Progress::new(("provider-quota", id * 2 + index))
                                        .xsmall()
                                        .flex_1()
                                        .value(used.unwrap_or(0.))
                                        .accessibility_label(label)
                                        .when(used.is_some_and(|used| used > 80.), |bar| {
                                            bar.color(cx.theme().warning)
                                        }),
                                )
                                .child(div().w(px(30.)).flex_shrink_0().text_right().child(
                                    used.map_or_else(
                                        || tr("provider_usage_unknown").to_string(),
                                        |used| format!("{}%", used.round()),
                                    ),
                                )),
                        )
                    }),
            )
            .into_any_element()
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
