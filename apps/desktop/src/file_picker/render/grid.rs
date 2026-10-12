use super::*;
use gpui_kit::component::v_virtual_list;
use std::rc::Rc;
impl Picker {
    pub(super) fn grid(&self, width: Pixels, cx: &mut Context<Self>) -> AnyElement {
        let sizes = Rc::new(vec![
            size(width, px(104.));
            self.visible.len().div_ceil(self.columns)
        ]);
        v_virtual_list(
            cx.entity(),
            "directory-tiles",
            sizes,
            |picker, range, _, cx| {
                range
                    .map(|row| {
                        h_flex()
                            .w_full()
                            .h(px(104.))
                            .gap_1()
                            .items_start()
                            .children(
                                (row * picker.columns
                                    ..((row + 1) * picker.columns).min(picker.visible.len()))
                                    .map(|position| picker.tile(picker.visible[position], cx)),
                            )
                    })
                    .collect()
            },
        )
        .size_full()
        .track_scroll(&self.scroll)
        .into_any_element()
    }
    fn tile(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let entry = &self.entries()[index];
        div()
            .id(("directory-tile", index))
            .w(relative(1. / self.columns as f32))
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
                    .h(px(100.))
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
                            .child(super::list::icon(entry, px(44.), cx))
                            .child(
                                div()
                                    .w_full()
                                    .text_sm()
                                    .text_center()
                                    .whitespace_normal()
                                    .line_clamp(2)
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
