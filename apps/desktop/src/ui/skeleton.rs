//! Stable card placeholders composed from Kit Skeleton and the shared row layout.
use super::card_list::{self, Column};
use gpui_kit::{
    component::{skeleton::Skeleton, *},
    *,
};

pub(crate) fn list(id: impl Into<String>, rows: usize, cx: &App) -> AnyElement {
    let id = id.into();
    let line_height = |font_size| {
        TextStyle {
            font_size: rems(font_size).into(),
            ..TextStyle::default()
        }
        .line_height_in_pixels(cx.theme().font_size)
    };
    let title_height = line_height(0.875);
    let subtitle_height = line_height(0.75);
    let children = (0..rows.clamp(1, 20))
        .map(|index| {
            let row = format!("{id}-{index}");
            card_list::row(
                row.clone(),
                format!("{row}-content"),
                vec![
                    card_list::column(
                        format!("{row}-summary"),
                        Column::Content,
                        vec![
                            v_flex()
                                .gap_1()
                                .child(
                                    Skeleton::new()
                                        .h(title_height)
                                        .w_48()
                                        .max_w_full()
                                        .rounded_md(),
                                )
                                .child(
                                    Skeleton::new()
                                        .secondary()
                                        .h(subtitle_height)
                                        .w_32()
                                        .max_w_full()
                                        .rounded_md(),
                                )
                                .into_any_element(),
                        ],
                        cx,
                    )
                    .into_any_element(),
                    card_list::column(
                        format!("{row}-actions"),
                        Column::Actions { regular: false },
                        vec![Skeleton::new().size_5().rounded_md().into_any_element()],
                        cx,
                    )
                    .into_any_element(),
                ],
                cx,
            )
            .into_any_element()
        })
        .collect();
    card_list::list(id, children).into_any_element()
}
