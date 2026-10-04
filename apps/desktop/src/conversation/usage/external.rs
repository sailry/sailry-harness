//! Declared metrics use the same typography and flat details as native statistics.
use super::*;
use crate::plugins::contributions::Entry;

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
                .gap_1p5()
                .text_sm()
                .line_height(px(20.))
                .text_color(cx.theme().muted_foreground)
                .children(entries.iter().flat_map(rows))
                .into_any_element()
        }),
    }
}

fn label(entry: &sailry_protocol::plugin::desktop::Navigation) -> SharedString {
    entry.label(&rust_i18n::locale()).to_owned().into()
}

fn rows(entry: &Entry) -> Vec<Div> {
    let mut rows = vec![row(
        format!(
            "statistics-plugin-{}-{}",
            entry.key.package.name, entry.key.id
        ),
        label(&entry.declaration.label),
        entry.value(),
    )];
    rows.extend(
        entry
            .state
            .details
            .iter()
            .enumerate()
            .map(|(index, detail)| {
                row(
                    format!(
                        "statistics-plugin-{}-{}-{index}",
                        entry.key.package.name, entry.key.id
                    ),
                    label(&detail.label),
                    detail.value.clone().into(),
                )
            }),
    );
    rows
}

fn row(id: String, label: SharedString, value: SharedString) -> Div {
    h_flex()
        .w_full()
        .gap_4()
        .justify_between()
        .debug_selector(move || id.clone())
        .child(label)
        .child(div().flex_shrink_0().child(value))
}

fn details(entry: &Entry, cx: &App) -> AnyElement {
    v_flex()
        .debug_selector(|| "composer-stat-details".into())
        .w(px(220.))
        .p_2()
        .gap_2()
        .text_sm()
        .line_height(px(20.))
        .text_color(cx.theme().muted_foreground)
        .children(rows(entry))
        .into_any_element()
}
