//! Shared channel-brand matrix for the welcome guide and provider editor.
use super::presets;
use gpui_kit::{
    base::{
        animation::cubic_bezier,
        motion::{self, Transition},
    },
    component::{
        button::{Button, ButtonCustomVariant, ButtonVariants},
        *,
    },
    *,
};
use sailry_protocol::conversation::presets::Entry;
use std::{collections::BTreeMap, rc::Rc, time::Duration};

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

/// Distance of each tile from the matrix centre in tile steps. Rows are centred.
fn distances(lengths: &[usize]) -> Vec<f32> {
    let middle = (lengths.len() as f32 - 1.) / 2.;
    lengths
        .iter()
        .enumerate()
        .flat_map(|(row, &length)| {
            let y = row as f32 - middle;
            (0..length).map(move |column| (column as f32 - (length as f32 - 1.) / 2.).hypot(y))
        })
        .collect()
}

/// Fill alpha and edge strength, fading outward from the centre.
fn fade(distance: f32) -> (f32, f32) {
    (
        (0.64 - 0.095 * distance).max(0.22),
        (1. - 0.16 * distance).max(0.35),
    )
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
        let glyph = 1.75 * f32::from(tile) / f32::from(TILE);
        let mut tiles = Vec::new();
        for (((key, choices), handle), distance) in self
            .groups
            .into_iter()
            .zip(&handles)
            .zip(distances(&lengths))
        {
            let entry = &choices[0];
            let label = presets::brand_label(entry);
            let hover = window.use_keyed_state(
                SharedString::from(format!("provider-brand-hover-{key}")),
                cx,
                |_, _| false,
            );
            let target = if *hover.read(cx) && !self.disabled {
                1.
            } else {
                0.
            };
            let progress = motion::transition(
                format!("provider-brand-lift-{key}"),
                target,
                Transition::new(Duration::from_millis(350)).ease(cubic_bezier(0.22, 1., 0.36, 1.)),
                window,
                cx,
            );
            let (alpha, edge) = fade(distance);
            // Kit's hover color follows the animated fill so it does not jump.
            let fill = cx
                .theme()
                .secondary
                .opacity(alpha + (1. - alpha) * progress);
            let select = self.select.clone();
            tiles.push(
                Button::new(SharedString::from(format!("provider-brand-{key}")))
                    .custom(
                        ButtonCustomVariant::new(cx)
                            .color(fill)
                            .foreground(cx.theme().foreground)
                            .hover(fill)
                            .active(cx.theme().list_active),
                    )
                    .size(tile)
                    .p_0()
                    .border_1()
                    .border_color(cx.theme().border.opacity(0.7 + 0.3 * edge))
                    .rounded(cx.theme().radius_lg)
                    // The theme has no shadow color token; a black drop shadow sits below the tile.
                    .shadow(vec![
                        BoxShadow::new(
                            px(0.),
                            px(6.),
                            gpui_kit::black().opacity(0.24 * (0.65 + 0.35 * edge)),
                        )
                        .blur_radius(px(7.))
                        .spread_radius(px(-5.)),
                    ])
                    .disabled(self.disabled)
                    .track_focus(handle)
                    .accessibility_label(label.clone())
                    .tooltip(label)
                    .tooltip_placement(Placement::Top)
                    .child(presets::render_icon(
                        entry,
                        rems(glyph * (1. + 0.18 * progress)),
                    ))
                    .on_hover(move |hovered, _, cx| {
                        hover.update(cx, |hover, cx| {
                            *hover = *hovered;
                            cx.notify();
                        })
                    })
                    .on_click(move |_, window, cx| select(key.clone(), window, cx)),
            );
        }
        let mut tiles = tiles.into_iter();
        let rows = lengths
            .iter()
            .map(|length| {
                h_flex()
                    .justify_center()
                    .gap(GAP)
                    .children(tiles.by_ref().take(*length))
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
    fn fades_outward_from_centre() {
        let rows = row_lengths(14);
        let distances = distances(&rows);
        assert_eq!(distances.len(), 14);
        assert!((0..14).all(|index| distances[index] == distances[13 - index]));
        assert_eq!(distances[6], 0.5);
        assert_eq!(distances[0], 1.5f32.hypot(1.));
        assert_eq!(fade(0.), (0.64, 1.));
        assert_eq!(fade(10.), (0.22, 0.35));
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
