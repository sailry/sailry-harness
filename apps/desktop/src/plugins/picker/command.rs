//! Command rows share the native searchable List and retain their detail layout.
use super::*;

pub(super) fn row(item: ListItem, row: Item, height: f32, cx: &App) -> ListItem {
    let selector = row.id.clone();
    item.h(px(height)).child(
        h_flex()
            .debug_selector(move || selector.clone())
            .w_full()
            .min_w_0()
            .gap_2()
            .children(row.icon.map(|icon| {
                Icon::empty()
                    .path(if icon.contains('/') {
                        icon
                    } else {
                        format!("icons/{icon}.svg")
                    })
                    .size_4()
            }))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .line_height(relative(1.25))
                    .child(div().truncate().child(row.label))
                    .when(!row.detail.is_empty(), |column| {
                        column.child(
                            div()
                                .truncate()
                                .text_color(cx.theme().muted_foreground)
                                .child(row.detail),
                        )
                    }),
            )
            .when(row.submenu, |item| {
                item.child(Icon::new(IconName::ChevronRight).size_4())
            }),
    )
}
