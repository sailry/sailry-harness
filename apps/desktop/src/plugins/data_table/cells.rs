//! The retained DataTable SDK previously exposed only scalar cells. Kit's native
//! TableDelegate supports composed cells and Sizable supports custom row heights.
//! These small presentation types keep exact scalar rows available to selection,
//! copying and dump consumers without replacing Kit's table or scroll behavior.
//! Kit has no inline cell parts for monograms, trailing notes or chips;
//! single-line cells compose those parts and reuse Kit's native progress bar.
use super::*;
use gpui_kit::component::progress::Progress;

#[derive(Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct Cell {
    pub primary: Line,
    pub secondary: Option<Line>,
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct Line {
    pub text: String,
    pub icon: Option<String>,
    /// Icon color.
    pub tone: Option<Tone>,
    /// Text color; secondary lines default to muted text.
    pub text_tone: Option<Tone>,
    /// Leading one or two character monogram on a neutral surface.
    pub badge: Option<String>,
    /// Icon shown on the badge surface in place of the monogram.
    pub badge_icon: Option<String>,
    /// Muted text following the main text on the same line.
    pub note: Option<String>,
    /// Bordered muted label following the text.
    pub chip: Option<String>,
    pub meter: Option<Meter>,
}

/// Trailing proportion bar with its formatted value.
#[derive(Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct Meter {
    /// Percentage from 0 to 100.
    pub value: f32,
    pub label: String,
    pub tone: Option<Tone>,
    pub icon: Option<String>,
    pub width: Option<f32>,
    /// Stretches the bar across the remaining line width; `width` is then ignored.
    #[serde(default)]
    pub fill: bool,
}

#[derive(Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Tone {
    Primary,
    Muted,
    Warning,
    #[serde(rename = "chart_2")]
    Chart2,
    #[serde(rename = "chart_3")]
    Chart3,
    #[serde(rename = "chart_4")]
    Chart4,
}

impl Tone {
    fn color(self, cx: &App) -> Hsla {
        match self {
            Self::Primary => cx.theme().primary,
            Self::Muted => cx.theme().muted_foreground,
            Self::Warning => cx.theme().warning,
            Self::Chart2 => cx.theme().chart_2,
            Self::Chart3 => cx.theme().chart_3,
            Self::Chart4 => cx.theme().chart_4,
        }
    }
}

impl Meter {
    pub fn valid(&self) -> bool {
        (0. ..=100.).contains(&self.value)
            && self
                .width
                .is_none_or(|width| width.is_finite() && (16. ..=240.).contains(&width))
    }

    fn render(&self, id: &str, cx: &App) -> Div {
        let color = self
            .tone
            .map_or(cx.theme().muted_foreground, |tone| tone.color(cx));
        let id = id.to_owned();
        let bar = Progress::new(id).xsmall().value(self.value).color(color);
        h_flex()
            .when(self.fill, |meter| meter.flex_1().min_w_0())
            .when(!self.fill, |meter| meter.flex_shrink_0())
            .items_center()
            .gap_1()
            .child(if self.fill {
                div().flex_1().min_w_0().child(bar.w_full())
            } else {
                div().child(bar.w(px(self.width.unwrap_or(36.))))
            })
            .child(
                h_flex()
                    .min_w(px(36.))
                    .justify_end()
                    .items_center()
                    .gap_0p5()
                    .text_color(color)
                    .when_some(self.icon.clone(), |label, path| {
                        label.child(Icon::default().path(path).size_3().text_color(color))
                    })
                    .child(self.label.clone()),
            )
    }
}

impl Line {
    pub fn valid(&self) -> bool {
        self.meter.as_ref().is_none_or(Meter::valid)
            && self
                .badge
                .as_ref()
                .is_none_or(|badge| badge.chars().count() <= 2)
    }

    fn render(
        &self,
        id: String,
        secondary: bool,
        alignment: Alignment,
        compact: bool,
        cx: &App,
    ) -> Stateful<Div> {
        let text = self.text.clone();
        let muted = cx.theme().muted_foreground;
        let color = self.text_tone.map(|tone| tone.color(cx));
        h_flex()
            .id(SharedString::from(id.clone()))
            .debug_selector({
                let id = id.clone();
                move || id.clone()
            })
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .text_sm()
            .h_4()
            .line_height(px(16.))
            .when(compact, |line| line.h(px(14.)).line_height(px(14.)))
            .items_center()
            .gap_1()
            .when(alignment == Alignment::End, |line| line.justify_end())
            .when(secondary, |line| line.text_xs().text_color(muted))
            .when(self.badge.is_some() || self.badge_icon.is_some(), |line| {
                line.child(
                    div()
                        .flex_shrink_0()
                        .size(px(18.))
                        .mr_1()
                        .rounded(px(5.))
                        .bg(cx.theme().muted)
                        .text_color(muted)
                        .text_xs()
                        .font_semibold()
                        .flex()
                        .items_center()
                        .justify_center()
                        .map(|badge| match (&self.badge_icon, &self.badge) {
                            (Some(path), _) => badge.child(
                                Icon::default()
                                    .path(path.clone())
                                    .size_3()
                                    .text_color(cx.theme().foreground),
                            ),
                            (None, Some(text)) => badge.child(text.clone()),
                            (None, None) => badge,
                        }),
                )
            })
            .when_some(self.icon.clone(), |line, path| {
                line.child(Icon::default().path(path).size_3().text_color(
                    self.tone.map(|tone| tone.color(cx)).unwrap_or_else(|| {
                        if secondary {
                            muted
                        } else {
                            cx.theme().foreground
                        }
                    }),
                ))
            })
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .when_some(color, |text, color| text.text_color(color))
                    .child(text),
            )
            .when_some(self.note.clone(), |line, note| {
                line.child(
                    div()
                        .ml_1()
                        .min_w_0()
                        .truncate()
                        .text_color(muted)
                        .child(note),
                )
            })
            .when_some(self.chip.clone(), |line, chip| {
                line.child(
                    div()
                        .ml_1()
                        .flex_shrink_0()
                        .max_w(px(120.))
                        .h(px(18.))
                        .px_1p5()
                        .flex()
                        .items_center()
                        .rounded(px(4.))
                        .border_1()
                        .border_color(cx.theme().border)
                        .text_xs()
                        .text_color(muted)
                        .child(div().min_w_0().truncate().child(chip)),
                )
            })
            .when_some(self.meter.as_ref(), |line, meter| {
                line.child(meter.render(&format!("{id}-meter"), cx))
            })
    }
}

impl Cell {
    pub fn valid(&self) -> bool {
        self.primary.valid() && self.secondary.as_ref().is_none_or(Line::valid)
    }

    pub fn render(&self, id: &str, alignment: Alignment, compact: bool, cx: &App) -> Div {
        v_flex()
            .size_full()
            .min_w_0()
            .justify_center()
            .when(!compact && self.secondary.is_some(), |cell| cell.gap_1())
            .child(
                self.primary
                    .render(format!("{id}-primary"), false, alignment, compact, cx),
            )
            .when_some(self.secondary.as_ref(), |cell, line| {
                cell.child(line.render(format!("{id}-secondary"), true, alignment, compact, cx))
            })
    }
}
