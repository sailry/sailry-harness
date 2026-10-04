//! Shared semantic actions; document and tree surfaces route them to their captured owners.
use crate::shell::Shell;
use gpui_kit::*;

actions!(
    sailry_files,
    [
        SaveFile,
        CopySelection,
        CutSelection,
        PasteSelection,
        RenameSelection,
        SelectFiles,
        ExtendFilesUp,
        ExtendFilesDown
    ]
);

pub(super) fn init(cx: &mut App) {
    // Shared editor/tree conventions, not configurable plugin commands.
    cx.bind_keys([
        KeyBinding::new("secondary-s", SaveFile, Some("FileEditor")),
        KeyBinding::new("secondary-c", CopySelection, Some("FileTree > Tree")),
        KeyBinding::new("secondary-x", CutSelection, Some("FileTree > Tree")),
        KeyBinding::new("secondary-v", PasteSelection, Some("FileTree > Tree")),
        KeyBinding::new("secondary-a", SelectFiles, Some("FileTree > Tree")),
        KeyBinding::new("shift-up", ExtendFilesUp, Some("FileTree > Tree")),
        KeyBinding::new("shift-down", ExtendFilesDown, Some("FileTree > Tree")),
        KeyBinding::new("f2", RenameSelection, Some("FileTree > Tree")),
        KeyBinding::new(
            "enter",
            gpui_kit::base::actions::Confirm { secondary: false },
            Some("FileTree > Tree"),
        ),
    ]);
}

impl Shell {
    pub(crate) fn close_file_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.guard_document_navigation(
            window,
            cx,
            std::rc::Rc::new(|shell, window, cx| {
                shell.close_resource_panel(cx);
                shell.focus.focus(window, cx);
            }),
        ) {
            return;
        }
        self.close_resource_panel(cx);
        self.focus.focus(window, cx);
    }

    pub(crate) fn guard_file_navigation(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        next: impl Fn(&mut Shell, &mut Window, &mut Context<Self>) + 'static,
    ) -> bool {
        self.guard_document_navigation(window, cx, std::rc::Rc::new(next))
    }
}
