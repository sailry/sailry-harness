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
        self.commit(cx);
        self.dragging = false;
        self.focus.focus(window, cx);
        let selected = self.can_copy(cx);
        NativeMenu::new()
            .menu_with_disabled(crate::tr("files_undo"), self.readonly, Box::new(Undo))
            .menu_with_disabled(crate::tr("files_redo"), self.readonly, Box::new(Redo))
            .separator()
            .menu_with_disabled(crate::tr("files_copy"), !selected, Box::new(Copy))
            .menu_with_disabled(
                crate::tr("files_cut"),
                self.readonly || !selected,
                Box::new(Cut),
            )
            .menu_with_disabled(crate::tr("files_paste"), self.readonly, Box::new(Paste))
            .show(event.position, window, cx);
        cx.stop_propagation();
    }
}
