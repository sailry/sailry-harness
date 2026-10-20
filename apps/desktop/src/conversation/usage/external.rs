//! Declared metrics use the same typography and flat details as native statistics.
use super::*;
use crate::plugins::contributions::Entry;
use gpui_kit::component::progress::Progress;
use gpui_kit::prelude::FluentBuilder as _;
use sailry_protocol::plugin::{desktop::Navigation, ui::Detail};
use std::sync::Arc;

pub(in crate::conversation) fn group(entries: Vec<Entry>) -> Group {
    let metrics = entries
        .iter()
        .map(|entry| {
            let snapshot = entry.clone();
            Metric {
                id: format!("plugin-stat-{}-{}", entry.key.package.name, entry.key.id),
                order: entry.declaration.order,
                value: entry.value(),
                icon: entry.icon(),
                prefix: None,
                details: Rc::new(move |cx| details(&snapshot, cx)),
            }
        })
        .collect();
    Group {
        metrics,
        summary: Rc::new(move |cx| {
            v_flex()
                .w_full()
                .min_w_0()
                .gap_3()
                .children(entries.iter().map(|entry| body(entry, cx)))
                .into_any_element()
        }),
    }
}

fn label(entry: &Navigation) -> SharedString {
    entry.label(&rust_i18n::locale()).to_owned().into()
}

/// Rows without progress precede meters; each group keeps the declared order and index.
struct Layout<'a> {
    values: Vec<(usize, &'a Detail)>,
    meters: Vec<(usize, &'a Detail, f32)>,
}

impl<'a> Layout<'a> {
    fn new(details: &'a [Detail]) -> Self {
        let mut layout = Self {
            values: Vec::new(),
            meters: Vec::new(),
        };
        for (index, detail) in details.iter().enumerate() {
            match progress(detail) {
                Some(value) => layout.meters.push((index, detail, value)),
                None => layout.values.push((index, detail)),
            }
        }
        layout
    }
}

/// A detail's finite percentage, clamped for meters.
pub(crate) fn progress(detail: &Detail) -> Option<f32> {
    detail
        .progress
        .as_ref()
        .and_then(serde_json::Number::as_f64)
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(0., 100.) as f32)
}

pub(crate) fn tabular() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".into(), 1)]))
}

/// Labels yield space and truncate so the value beside them stays whole.
fn caption(text: SharedString) -> Div {
    div().flex_1().min_w_0().truncate().child(text)
}

fn amount(text: SharedString) -> Div {
    div()
        .flex_shrink_0()
        .whitespace_nowrap()
        .font_features(tabular())
        .child(text)
}

fn value_row(id: String, name: SharedString, value: SharedString, cx: &App) -> Div {
    h_flex()
        .w_full()
        .min_w_0()
        .h(px(26.))
        .gap_4()
        .text_sm()
        .debug_selector(move || id.clone())
        .child(caption(name).text_color(cx.theme().muted_foreground))
        .child(amount(value).text_color(cx.theme().foreground))
}

fn meter(id: String, name: SharedString, value: SharedString, progress: f32, cx: &App) -> Div {
    let bar = Progress::new(SharedString::from(format!("{id}-progress")))
        .value(progress)
        .xsmall()
        .accessibility_label(name.clone());
    v_flex()
        .w_full()
        .min_w_0()
        .gap(px(6.))
        .debug_selector(move || id.clone())
        .child(
            h_flex()
                .w_full()
                .min_w_0()
                .gap_3()
                .child(caption(name).text_sm().text_color(cx.theme().foreground))
                .child(
                    amount(value)
                        .text_xs()
                        .text_color(cx.theme().muted_foreground),
                ),
        )
        .child(bar)
}

fn body(entry: &Entry, cx: &App) -> Div {
    let id = format!(
        "statistics-plugin-{}-{}",
        entry.key.package.name, entry.key.id
    );
    let summary = (label(&entry.declaration.label), entry.value());
    content(id, Some(summary), &entry.state.details, cx)
}

/// Summary rows lead, then a separator and the meters.
fn content(
    id: String,
    summary: Option<(SharedString, SharedString)>,
    details: &[Detail],
    cx: &App,
) -> Div {
    let layout = Layout::new(details);
    let divided = summary.is_some() || !layout.values.is_empty();
    let values = v_flex()
        .w_full()
        .min_w_0()
        .when_some(summary, |values, (name, value)| {
            values.child(value_row(id.clone(), name, value, cx))
        });
    let values = values.children(layout.values.iter().map(|(index, detail)| {
        value_row(
            format!("{id}-{index}"),
            label(&detail.label),
            detail.value.clone().into(),
            cx,
        )
    }));
    let meters = (!layout.meters.is_empty()).then(|| {
        v_flex()
            .w_full()
            .min_w_0()
            .gap_3()
            .children(layout.meters.iter().map(|(index, detail, progress)| {
                meter(
                    format!("{id}-{index}"),
                    label(&detail.label),
                    detail.value.clone().into(),
                    *progress,
                    cx,
                )
            }))
    });
    v_flex()
        .w_full()
        .min_w_0()
        .child(values)
        .when_some(meters, |body, meters| {
            body.when(divided, |body| {
                body.child(div().mt(px(10.)).mb(px(12.)).child(Separator::horizontal()))
            })
            .child(meters)
        })
}

/// The popover content width for a declared surface width.
fn frame(width: Option<u16>) -> Div {
    div()
        .w(px(
            width.map_or(POPOVER_WIDTH, f32::from) - 2. * POPOVER_INSET.1
        ))
        .min_w_0()
}

fn details(entry: &Entry, cx: &App) -> AnyElement {
    frame(entry.declaration.width)
        .debug_selector(|| "composer-stat-details".into())
        .child(body(entry, cx))
        .into_any_element()
}

/// Every entry in one popover, sized by the first entry's declared width.
pub(crate) fn panel(entries: &[Entry], cx: &App) -> AnyElement {
    frame(entries.first().and_then(|entry| entry.declaration.width))
        .child(
            v_flex()
                .w_full()
                .min_w_0()
                .gap_3()
                .children(entries.iter().enumerate().map(|(index, entry)| {
                    v_flex()
                        .w_full()
                        .gap_3()
                        .when(index > 0, |section| section.child(Separator::horizontal()))
                        .child(body(entry, cx))
                })),
        )
        .into_any_element()
}

/// Host-owned details without a summary row.
pub(crate) fn rows(id: String, details: &[Detail], cx: &App) -> AnyElement {
    frame(None)
        .child(content(id, None, details, cx))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{Detail, Layout, amount, caption};
    use gpui_kit::{Overflow, Styled as _, TextOverflow, WhiteSpace, px};
    use serde_json::json;

    fn details(value: serde_json::Value) -> Vec<Detail> {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn groups_values_before_meters_in_order() {
        let details = details(json!([
            {"label":{"label":"A"},"value":"1"},
            {"label":{"label":"B"},"value":"2","progress":40},
            {"label":{"label":"C"},"value":"3"},
            {"label":{"label":"D"},"value":"4","progress":95.5}
        ]));
        let layout = Layout::new(&details);
        let values: Vec<_> = layout
            .values
            .iter()
            .map(|(_, row)| row.value.as_str())
            .collect();
        let meters: Vec<_> = layout
            .meters
            .iter()
            .map(|(index, row, progress)| (*index, row.value.as_str(), *progress))
            .collect();
        assert_eq!(values, ["1", "3"]);
        assert_eq!(meters, [(1, "2", 40.), (3, "4", 95.5)]);
    }

    #[test]
    fn clamps_progress() {
        let details = details(json!([
            {"label":{"label":"Low"},"value":"0","progress":-5},
            {"label":{"label":"High"},"value":"9","progress":250}
        ]));
        let progress: Vec<_> = Layout::new(&details)
            .meters
            .iter()
            .map(|(_, _, progress)| *progress)
            .collect();
        assert_eq!(progress, [0., 100.]);
    }

    #[test]
    fn labels_truncate_and_values_keep_width() {
        let mut name = caption("A very long package name".into());
        let style = name.style();
        assert_eq!(style.flex_grow, Some(1.));
        assert_eq!(style.min_size.width, Some(px(0.).into()));
        assert_eq!(style.overflow.x, Some(Overflow::Hidden));
        assert_eq!(style.text.white_space, Some(WhiteSpace::Nowrap));
        assert!(matches!(
            style.text.text_overflow,
            Some(TextOverflow::Truncate(_))
        ));
        let mut value = amount("1500 / 1500".into());
        let style = value.style();
        assert_eq!(style.flex_shrink, Some(0.));
        assert_eq!(style.text.white_space, Some(WhiteSpace::Nowrap));
        assert_eq!(style.overflow.x, None);
    }
}
