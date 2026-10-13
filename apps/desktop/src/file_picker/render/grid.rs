use super::*;
use gpui_kit::component::v_virtual_list;
use std::rc::Rc;
impl Picker {
    pub(super) fn grid(
        &self,
        width: Pixels,
        gap: Pixels,
        rem: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let height = rem * 6.5;
        let tile_width =
            (width - gap * self.columns.saturating_sub(1) as f32) / self.columns as f32;
        let sizes = Rc::new(vec![
            size(width, height);
            self.visible.len().div_ceil(self.columns)
        ]);
        v_virtual_list(
            cx.entity(),
            "directory-tiles",
            sizes,
            move |picker, range, _, cx| {
                range
                    .map(|row| {
                        h_flex().w_full().h(height).gap_2().items_start().children(
                            (row * picker.columns
                                ..((row + 1) * picker.columns).min(picker.visible.len()))
                                .map(|position| {
                                    picker.tile(
                                        picker.visible[position],
                                        tile_width,
                                        height - gap,
                                        cx,
                                    )
                                }),
                        )
                    })
                    .collect()
            },
        )
        .size_full()
        .track_scroll(&self.scroll)
        .into_any_element()
    }
    fn tile(
        &self,
        index: usize,
        width: Pixels,
        height: Pixels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let entry = &self.entries()[index];
        div()
            .id(("directory-tile", index))
            .w(width)
            .flex_shrink_0()
            .min_w_0()
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |picker, event, window, cx| {
                    picker.menu(Some(index), event, window, cx)
                }),
            )
            .child(
                Button::new(("directory-entry", index))
                    .custom(crate::theme::subtle_button(cx))
                    .w_full()
                    .h(height)
                    .p_2()
                    .selected(self.selected.contains(&index))
                    .disabled(!matches!(
                        entry.kind,
                        EntryKind::Directory | EntryKind::File
                    ))
                    .accessibility_label(entry.name.clone())
                    .debug_selector(move || format!("directory-row-{index}"))
                    .when(entry.name.starts_with('.'), |button| button.opacity(0.55))
                    .child(
                        v_flex()
                            .size_full()
                            .items_center()
                            .justify_center()
                            .gap_2()
                            .child(super::list::icon(entry, rems(2.), cx))
                            .child(
                                div()
                                    .w_full()
                                    .debug_selector(move || format!("directory-label-{index}"))
                                    .min_w_0()
                                    .text_xs()
                                    .text_center()
                                    .whitespace_normal()
                                    .line_clamp(2)
                                    .text_ellipsis()
                                    .overflow_hidden()
                                    .text_color(if entry.kind == EntryKind::Directory {
                                        cx.theme().foreground
                                    } else {
                                        cx.theme().muted_foreground
                                    })
                                    .child(entry.name.clone()),
                            ),
                    )
                    .on_click(cx.listener(move |picker, event: &ClickEvent, window, cx| {
                        picker.select(index, window.modifiers(), window, cx);
                        if event.click_count() == 2 {
                            picker.activate(index, window, cx);
                        }
                    })),
            )
    }
}
