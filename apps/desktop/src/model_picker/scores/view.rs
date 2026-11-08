use super::*;
use gpui_kit::component::{menu::PopupMenuItem, tooltip::Tooltip, *};

pub(in crate::model_picker) fn effort_content(
    label: SharedString,
    score: Option<f32>,
    cx: &App,
) -> Div {
    h_flex()
        .w_full()
        .gap_4()
        .child(div().flex_1().child(label))
        .children(score.map(|score| {
            div()
                .font_family(cx.theme().mono_font_family.clone())
                .text_color(cx.theme().muted_foreground)
                .child(format!("{score:.1}"))
        }))
}

pub(crate) fn effort_item(label: SharedString, score: Option<f32>, _: &App) -> PopupMenuItem {
    PopupMenuItem::element(move |_, cx| effort_content(label.clone(), score, cx))
}

impl Measured {
    fn tooltip(self) -> Tooltip {
        Tooltip::element(move |_, cx| self.details(cx)).p_3()
    }

    fn details(&self, cx: &App) -> AnyElement {
        v_flex()
            .w(rems(24.))
            .gap_1()
            .text_sm()
            .child(ring::render(&self.score, cx))
            .when(self.preview, |card| {
                card.child(
                    div()
                        .text_center()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(tr("model_score_preview")),
                )
            })
            .into_any_element()
    }
}

impl Picker {
    pub fn score_tooltip(&self, effort: Effort, cx: &App) -> Option<Tooltip> {
        let (group, model) = self.selected_choice()?;
        Some(self.measured(group, model, effort, cx)?.tooltip())
    }

    pub(in crate::model_picker) fn score_tag(
        &self,
        group: &Group,
        model: &Choice,
        effort: Effort,
        cx: &App,
    ) -> Option<AnyElement> {
        let measured = self.measured(group, model, effort, cx)?;
        let score = format!("{:.1}", measured.score.overall);
        Some(
            div()
                .id(SharedString::from(format!(
                    "model-score-{}-{}",
                    group.id, model.id
                )))
                .flex_shrink_0()
                .font_family(cx.theme().mono_font_family.clone())
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(score)
                .tooltip(move |window, cx| measured.clone().tooltip().build(window, cx))
                .into_any_element(),
        )
    }
}
