use super::{View, protocol};
use crate::tr;
use gpui_kit::{component::native_menu::NativeMenu, *};
use std::ops::Range;

actions!(terminal, [Copy, Paste, SelectAll]);

#[derive(Default)]
pub(super) struct Composition {
    pub text: String,
    selected: Range<usize>,
}

impl View {
    pub(super) fn key_down(
        &mut self,
        event: &KeyDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.controlling() {
            return;
        }
        if let Some(event) = self
            .keyboard
            .press(event, !self.composition.text.is_empty())
        {
            self.input(protocol::Input::Key { event }, cx);
            cx.stop_propagation();
        }
    }
    pub(super) fn key_up(&mut self, event: &KeyUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(event) = self.keyboard.release(&event.keystroke) {
            self.input(protocol::Input::Key { event }, cx);
            cx.stop_propagation();
        }
    }
    pub(super) fn modifiers_changed(
        &mut self,
        _: &ModifiersChangedEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.notify();
        if self.controlling()
            && self.focused
            && let Some(event) = self.keyboard.modifier()
        {
            self.input(protocol::Input::Key { event }, cx);
        }
    }
    pub(super) fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(snapshot) = &self.state.snapshot {
            let text = self.selection.text(&snapshot.screen);
            if !text.is_empty() {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
        }
    }
    pub(super) fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text())
            && !text.is_empty()
        {
            if text.len() > protocol::MAX_INPUT_BYTES {
                crate::feedback::error("", &tr("terminal_paste_too_large"), window, cx);
                return;
            }
            if crate::preferences::data(cx).terminal.paste_protection && text.contains(['\n', '\r'])
            {
                let owner = cx.entity().downgrade();
                crate::prompts::confirm(
                    &tr("terminal_paste_confirm"),
                    &tr("terminal_paste_warning"),
                    tr("terminal_paste"),
                    window,
                    cx,
                    move |_, cx| {
                        let _ = owner.update(cx, |view, cx| {
                            view.input(protocol::Input::Paste { text }, cx)
                        });
                    },
                );
                return;
            }
            self.input(protocol::Input::Paste { text }, cx);
        }
    }
    pub(super) fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.selection.clear();
        if let Some(snapshot) = &self.state.snapshot {
            self.selection.anchor = Some(super::selection::Position::default());
            self.selection.end = Some(super::selection::Position {
                row: snapshot.screen.scrollback.len() + snapshot.screen.rows.len() - 1,
                column: snapshot.screen.columns,
            });
            cx.notify();
        }
    }
    pub(super) fn context_menu(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus.focus(window, cx);
        if self.report_press(event, cx) {
            return;
        }
        cx.stop_propagation();
        if cfg!(any(target_os = "macos", target_os = "windows")) {
            let empty = self
                .state
                .snapshot
                .as_ref()
                .is_none_or(|snapshot| self.selection.text(&snapshot.screen).is_empty());
            NativeMenu::new()
                .menu_with_disabled(tr("terminal_copy"), empty, Box::new(Copy))
                .menu_with_disabled(tr("terminal_paste"), !self.controlling(), Box::new(Paste))
                .separator()
                .menu(tr("terminal_select_all"), Box::new(SelectAll))
                .show(event.position, window, cx);
        } else {
            crate::feedback::error("", &tr("terminal_native_menu_unavailable"), window, cx);
        }
    }
}

fn byte_offset(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (offset, character) in text.char_indices() {
        if units >= utf16 {
            return offset;
        }
        units += character.len_utf16();
    }
    text.len()
}

impl EntityInputHandler for View {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let text = &self.composition.text;
        let start = byte_offset(text, range.start);
        let end = byte_offset(text, range.end.max(range.start));
        *actual = Some(text[..start].encode_utf16().count()..text[..end].encode_utf16().count());
        Some(text[start..end].into())
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        self.controlling().then(|| UTF16Selection {
            range: self.composition.selected.clone(),
            reversed: false,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        (!self.composition.text.is_empty()).then(|| 0..self.composition.text.encode_utf16().count())
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.composition = Default::default();
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.composition = Default::default();
        if let Some(input) = self.keyboard.commit(text) {
            self.input(input, cx);
        }
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.keyboard.composing();
        let old = &self.composition.text;
        let range = range.unwrap_or(0..old.encode_utf16().count());
        let start = byte_offset(old, range.start);
        let end = byte_offset(old, range.end.max(range.start));
        self.composition.text = format!("{}{}{}", &old[..start], text, &old[end..]);
        let length = self.composition.text.encode_utf16().count();
        let selected = selected.unwrap_or(length..length);
        self.composition.selected = selected.start.min(length)..selected.end.min(length);
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        use gpui_kit::component::scroll::ScrollbarHandle;
        let snapshot = self.state.snapshot.as_ref()?;
        let cursor = snapshot.screen.cursor?;
        Some(Bounds::new(
            point(
                self.metrics.bounds.left() + self.metrics.cell.width * cursor.column as f32,
                self.metrics.bounds.top()
                    + self.scroll.offset().y
                    + self.metrics.cell.height
                        * (snapshot.screen.scrollback.len() + cursor.row as usize) as f32,
            ),
            self.metrics.cell,
        ))
    }
    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.composition.selected.end)
    }
}
