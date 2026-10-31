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
        KeyBinding::load(
            "secondary-s",
            Box::new(SaveFile),
            Some(
                KeyBindingContextPredicate::parse("FileEditor")
                    .expect("static shortcut context")
                    .into(),
            ),
            false,
            None,
            cx,
        )
        .expect("static shortcut"),
        KeyBinding::load(
            "secondary-c",
            Box::new(CopySelection),
            Some(
                KeyBindingContextPredicate::parse("FileTree > Tree")
                    .expect("static shortcut context")
                    .into(),
            ),
            false,
            None,
            cx,
        )
        .expect("static shortcut"),
        KeyBinding::load(
            "secondary-x",
            Box::new(CutSelection),
            Some(
                KeyBindingContextPredicate::parse("FileTree > Tree")
                    .expect("static shortcut context")
                    .into(),
            ),
            false,
            None,
            cx,
        )
        .expect("static shortcut"),
        KeyBinding::load(
            "secondary-v",
            Box::new(PasteSelection),
            Some(
                KeyBindingContextPredicate::parse("FileTree > Tree")
                    .expect("static shortcut context")
                    .into(),
            ),
            false,
            None,
            cx,
        )
        .expect("static shortcut"),
        KeyBinding::load(
            "secondary-a",
            Box::new(SelectFiles),
            Some(
                KeyBindingContextPredicate::parse("FileTree > Tree")
                    .expect("static shortcut context")
                    .into(),
            ),
            false,
            None,
            cx,
        )
        .expect("static shortcut"),
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
            std::rc::Rc::new(|shell, window, cx| shell.dismiss_resource_panel(window, cx)),
        ) {
            return;
        }
        self.dismiss_resource_panel(window, cx);
    }

    /// Hides native surfaces before dropping the resource so none outlive its page.
    fn dismiss_resource_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(browser) = self
            .side_resource
            .as_ref()
            .and_then(|resource| resource.browser(cx))
        {
            browser.update(cx, |browser, cx| browser.visibility(false, window, cx));
        }
        if let Some(super::SideResource::CardBrowser(browser)) = &self.side_resource {
            browser.update(cx, |browser, cx| browser.hide(cx));
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
