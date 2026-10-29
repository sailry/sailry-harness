//! Shared channel-brand matrix for the welcome guide and provider editor.
use super::presets;
use gpui_kit::{
    component::{
        button::{Button, ButtonCustomVariant, ButtonVariants},
        *,
    },
    *,
};
use sailry_protocol::conversation::presets::Entry;
use std::{collections::BTreeMap, rc::Rc};

pub(super) const WIDTH: Pixels = px(640.);
const TILE: Pixels = px(76.);
const GAP: Pixels = px(12.);

fn row_lengths(count: usize) -> Vec<usize> {
    let mut remaining = count;
    [4, 6, 4]
        .into_iter()
        .cycle()
        .map_while(|capacity| {
            let length = remaining.min(capacity);
            remaining -= length;
            (length > 0).then_some(length)
        })
        .collect()
}

fn groups(entries: Option<&[Entry]>) -> Vec<(String, Vec<Entry>)> {
    let mut groups = presets::groups(&presets::available(entries));
    groups.sort_by_cached_key(|(key, choices)| {
        (
            presets::brand_label(&choices[0]).to_lowercase(),
            key.clone(),
        )
    });
    groups
}

/// Kit has no two-dimensional action group. Only focus routing is adapted here;
/// Button keeps keyboard activation, focus styling, disabled state and tooltips.
fn next(lengths: &[usize], current: usize, key: &str) -> Option<usize> {
    let mut start = 0;
    let (row, column) = lengths.iter().enumerate().find_map(|(row, length)| {
        let offset = start;
        start += length;
        (current < start).then_some((row, current - offset))
    })?;
    let length = lengths[row];
    let start = lengths[..row].iter().sum::<usize>();
    Some(match key {
        "left" => start + (column + length - 1) % length,
        "right" => start + (column + 1) % length,
        "home" => start,
        "end" => start + length - 1,
        "up" | "down" => {
            let adjacent =
                (row + if key == "down" { 1 } else { lengths.len() - 1 }) % lengths.len();
            let center = column as isize * 2 - length as isize + 1;
            // Compare centered tile positions, including the shorter edge rows.
            let target = (0..lengths[adjacent])
                .min_by_key(|column| {
                    ((column * 2) as isize - lengths[adjacent] as isize + 1 - center).abs()
                })
                .unwrap();
            lengths[..adjacent].iter().sum::<usize>() + target
        }
        _ => return None,
    })
}

pub(super) fn matrix(
    entries: Option<&[Entry]>,
    width: Pixels,
    disabled: bool,
    select: impl Fn(String, &mut Window, &mut App) + 'static,
) -> AnyElement {
    Matrix {
        groups: groups(entries),
        width,
        disabled,
        select: Rc::new(select),
    }
    .into_any_element()
}

type Select = Rc<dyn Fn(String, &mut Window, &mut App)>;

#[derive(IntoElement)]
struct Matrix {
    groups: Vec<(String, Vec<Entry>)>,
    width: Pixels,
    disabled: bool,
    select: Select,
}

impl RenderOnce for Matrix {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state("provider-brand-focus", cx, |_, _| {
            BTreeMap::<String, FocusHandle>::new()
        });
        let handles = state.update(cx, |handles, cx| {
            handles.retain(|key, _| self.groups.iter().any(|(brand, _)| brand == key));
            self.groups
                .iter()
                .map(|(key, _)| {
                    handles
                        .entry(key.clone())
                        .or_insert_with(|| cx.focus_handle())
                        .clone()
                })
                .collect::<Vec<_>>()
        });
        let lengths = row_lengths(self.groups.len());
        let tile = ((self.width - px(56.) - GAP * 5.) / 6.).clamp(px(36.), TILE);
        let glyph = rems(2.125 * f32::from(tile) / f32::from(TILE));
        let mut groups = self.groups.into_iter().zip(handles.iter());
        let rows = lengths
            .iter()
            .map(|length| {
                h_flex()
                    .justify_center()
                    .gap(GAP)
                    .children(
                        groups
                            .by_ref()
                            .take(*length)
                            .map(|((key, choices), handle)| {
                                let entry = &choices[0];
                                let label = presets::brand_label(entry);
                                let select = self.select.clone();
                                Button::new(SharedString::from(format!("provider-brand-{key}")))
                                    .custom(
                                        ButtonCustomVariant::new(cx)
                                            .color(cx.theme().secondary)
                                            .foreground(cx.theme().foreground)
                                            .hover(cx.theme().list_hover)
                                            .active(cx.theme().list_active),
                                    )
                                    .size(tile)
                                    .p_0()
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .rounded(cx.theme().radius_lg)
                                    .disabled(self.disabled)
                                    .track_focus(handle)
                                    .accessibility_label(label.clone())
                                    .tooltip(label)
                                    .tooltip_placement(Placement::Top)
                                    .child(presets::render_icon(entry, glyph))
                                    .on_click(move |_, window, cx| select(key.clone(), window, cx))
                            }),
                    )
            })
            .collect::<Vec<_>>();
        v_flex()
            .id("provider-brand-matrix")
            .w_full()
            .items_center()
            .gap(GAP)
            .children(rows)
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                if self.disabled || event.keystroke.modifiers != Modifiers::default() {
                    return;
                }
                let Some(current) = handles.iter().position(|handle| handle.is_focused(window))
                else {
                    return;
                };
                let Some(index) = next(&lengths, current, &event.keystroke.key) else {
                    return;
                };
                cx.stop_propagation();
                handles[index].focus(window, cx);
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn extends_rows_without_losing_brands() {
        assert_eq!(row_lengths(14), [4, 6, 4]);
        for count in [0, 1, 5, 14, 15, 33, 50] {
            let rows = row_lengths(count);
            assert_eq!(rows.iter().sum::<usize>(), count);
            assert!(rows.iter().all(|length| (1..=6).contains(length)));
        }
    }

    #[test]
    fn routes_focus_across_centered_rows() {
        let rows = row_lengths(14);
        assert_eq!(next(&rows, 0, "left"), Some(3));
        assert_eq!(next(&rows, 3, "right"), Some(0));
        assert_eq!(next(&rows, 6, "home"), Some(4));
        assert_eq!(next(&rows, 6, "end"), Some(9));
        assert_eq!(next(&rows, 0, "down"), Some(5));
        assert_eq!(next(&rows, 4, "down"), Some(10));
        assert_eq!(next(&rows, 13, "down"), Some(3));
        assert_eq!(next(&rows, 0, "up"), Some(10));
        assert_eq!(next(&rows, 1, "enter"), None);
        assert_eq!(next(&[], 0, "left"), None);
    }

    #[test]
    fn shares_declared_order_across_entry_points() {
        let mut entries = presets::bundled();
        let expected = groups(Some(&entries));
        entries.reverse();
        assert_eq!(
            groups(Some(&entries)),
            expected
                .into_iter()
                .map(|(key, mut choices)| {
                    choices.reverse();
                    (key, choices)
                })
                .collect::<Vec<_>>()
        );
        assert!(groups(Some(&[])).is_empty());
    }
}
