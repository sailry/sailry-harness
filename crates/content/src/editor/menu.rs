use super::{Copy, Cut, Paste, Redo, State, Undo};
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{Context, MouseDownEvent, Window};

impl State {
    pub(super) fn context_menu(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_composition(cx);
        self.dragging = false;
        self.focus.focus(window, cx);
        let selected = self.can_copy(cx);
        let labels = cx.global::<crate::Labels>();
        NativeMenu::new()
            .menu_with_disabled(labels.undo.clone(), self.readonly, Box::new(Undo))
            .menu_with_disabled(labels.redo.clone(), self.readonly, Box::new(Redo))
            .separator()
            .menu_with_disabled(labels.copy.clone(), !selected, Box::new(Copy))
            .menu_with_disabled(
                labels.cut.clone(),
                self.readonly || !selected,
                Box::new(Cut),
            )
            .menu_with_disabled(labels.paste.clone(), self.readonly, Box::new(Paste))
            .show(event.position, window, cx);
        cx.stop_propagation();
    }
}
