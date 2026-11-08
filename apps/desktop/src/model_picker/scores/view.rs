use super::*;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    menu::PopupMenuItem,
    progress::Progress,
    tag::Tag,
    tooltip::Tooltip,
    *,
};

pub(crate) fn effort_item(label: SharedString, score: Option<f32>, cx: &App) -> PopupMenuItem {
    let mono = cx.theme().mono_font_family.clone();
    let muted = cx.theme().muted_foreground;
    PopupMenuItem::element(move |_, _| {
        h_flex()
            .w_full()
            .gap_4()
            .child(div().flex_1().child(label.clone()))
            .child(
                div()
                    .font_family(mono.clone())
                    .text_color(muted)
                    .child(score.map_or_else(|| "—".to_owned(), |score| format!("{score:.1}"))),
            )
    })
}

impl Measured {
    fn source(&self) -> String {
        if self.preview {
            return tr("model_score_preview").to_string();
        }
        ["ModelDial".to_owned()]
            .into_iter()
            .chain(self.batch.clone())
            .chain(self.published.as_deref().map(date))
            .collect::<Vec<_>>()
            .join(" · ")
    }

    fn details(&self, name: &str, cx: &App) -> AnyElement {
        let axes = [
            "model_score_axis_backend",
            "model_score_axis_frontend",
            "model_score_axis_knowledge",
            "model_score_axis_aesthetic",
            "model_score_axis_maintainability",
        ];
        v_flex()
            .w_72()
            .gap_2()
            .text_xs()
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().min_w_0().child(format!(
                        "{name} · {}",
                        crate::reasoning::label(self.score.effort)
                    )))
                    .child(
                        div()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_lg()
                            .child(format!("{:.1}", self.score.overall)),
                    ),
            )
            .children(axes.into_iter().zip(self.score.axes).enumerate().map(
                |(index, (key, value))| {
                    h_flex()
                        .gap_2()
                        .child(div().w_20().child(tr(key)))
                        .child(
                            Progress::new(("model-score-axis", index))
                                .small()
                                .flex_1()
                                .value(f32::from(value)),
                        )
                        .child(
                            div()
                                .w_6()
                                .text_right()
                                .font_family(cx.theme().mono_font_family.clone())
                                .child(value.to_string()),
                        )
                },
            ))
            .when_some(self.score.elapsed_ms, |body, elapsed| {
                body.child(
                    div().text_color(cx.theme().muted_foreground).child(
                        rust_i18n::t!(
                            "model_score_time",
                            n = format!("{:.0}", elapsed as f64 / 60_000.)
                        )
                        .to_string(),
                    ),
                )
            })
            .when_some(self.score.cost_usd, |body, cost| {
                body.child(div().text_color(cx.theme().muted_foreground).child(
                    rust_i18n::t!("model_score_cost", cost = format!("${cost:.2}")).to_string(),
                ))
            })
            .child(
                div()
                    .pt_2()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_color(cx.theme().muted_foreground)
                    .child(self.source()),
            )
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr("model_score_modified")),
            )
            .into_any_element()
    }
}

pub(crate) fn date(value: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|date| date.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|_| value.to_owned())
}

impl Picker {
    pub(in crate::model_picker) fn score_tag(
        &self,
        group: &Group,
        model: &Choice,
        effort: Effort,
        cx: &App,
    ) -> Option<AnyElement> {
        let measured = self.measured(group, model, effort, cx)?;
        let score = format!("{:.1}", measured.score.overall);
        let label = model.label.clone();
        Some(
            div()
                .id(SharedString::from(format!(
                    "model-score-{}-{}",
                    group.id, model.id
                )))
                .flex_shrink_0()
                .child(
                    Tag::secondary()
                        .small()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_color(cx.theme().muted_foreground)
                        .child(score),
                )
                .tooltip(move |window, cx| {
                    let measured = measured.clone();
                    let label = label.clone();
                    Tooltip::element(move |_, cx| measured.details(&label, cx)).build(window, cx)
                })
                .into_any_element(),
        )
    }

    pub(in crate::model_picker) fn score_footer(&self, cx: &App) -> AnyElement {
        h_flex()
            .w_full()
            .flex_shrink_0()
            .gap_1()
            .pt_1()
            .px_2()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(div().flex_1().min_w_0().child(tr(if self.scores.preview {
                "model_score_preview"
            } else {
                "model_score_attribution"
            })))
            .when(!self.scores.preview, |footer| {
                footer.child(
                    Button::new("model-score-details")
                        .text()
                        .xsmall()
                        .label(tr("model_score_details"))
                        .tooltip(tr("model_score_modified"))
                        .on_click(|_, _, cx| cx.open_url(SOURCE)),
                )
            })
            .into_any_element()
    }
}
