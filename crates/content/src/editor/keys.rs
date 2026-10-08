//! Document editing keys adapted from Bezel 4a7505ab (MIT).
//! See third_party_licenses/bezel.md. Clipboard and history actions are Kit's.
use gpui_kit::component::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};
use gpui_kit::{App, Global, KeyBinding, actions};

pub(super) const CONTEXT: &str = "MarkdownEditor";
pub(super) const COMPOSER: &str = "MarkdownComposer";
pub(super) const SELECTION: &str = "MarkdownComposer MarkdownSelection";

actions!(
    markdown_editor,
    [
        Left,
        Right,
        Up,
        Down,
        Home,
        End,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectHome,
        SelectEnd,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        Backspace,
        Delete,
        Enter,
        SoftBreak,
        Indent,
        Outdent,
        Bold,
        Italic,
        Code,
        Strike,
    ]
);

struct Installed;
impl Global for Installed {}

/// Install once per application, including independent GPUI test applications.
pub(super) fn init(cx: &mut App) {
    if cx.try_global::<Installed>().is_some() {
        return;
    }
    cx.set_global(Installed);
    cx.bind_keys([
        KeyBinding::new(
            "secondary-enter",
            gpui_kit::component::input::Enter {
                secondary: true,
                shift: false,
            },
            Some(COMPOSER),
        ),
        KeyBinding::new("escape", gpui_kit::component::input::Escape, Some(COMPOSER)),
    ]);
    for name in [CONTEXT, COMPOSER] {
        let context = Some(name);
        let composer = name == COMPOSER;
        cx.bind_keys([
            KeyBinding::new("left", Left, context),
            KeyBinding::new("right", Right, context),
            if composer {
                KeyBinding::new("up", gpui_kit::component::input::MoveUp, context)
            } else {
                KeyBinding::new("up", Up, context)
            },
            if composer {
                KeyBinding::new("down", gpui_kit::component::input::MoveDown, context)
            } else {
                KeyBinding::new("down", Down, context)
            },
            KeyBinding::new("home", Home, context),
            KeyBinding::new("end", End, context),
            KeyBinding::new("shift-left", SelectLeft, context),
            KeyBinding::new("shift-right", SelectRight, context),
            KeyBinding::new("shift-up", SelectUp, context),
            KeyBinding::new("shift-down", SelectDown, context),
            KeyBinding::new("shift-home", SelectHome, context),
            KeyBinding::new("shift-end", SelectEnd, context),
            if composer {
                KeyBinding::new("backspace", gpui_kit::component::input::Backspace, context)
            } else {
                KeyBinding::new("backspace", Backspace, context)
            },
            if composer {
                KeyBinding::new(
                    "shift-backspace",
                    gpui_kit::component::input::Backspace,
                    context,
                )
            } else {
                KeyBinding::new("shift-backspace", Backspace, context)
            },
            KeyBinding::new("delete", Delete, context),
            if composer {
                KeyBinding::new(
                    "enter",
                    gpui_kit::component::input::Enter {
                        secondary: false,
                        shift: false,
                    },
                    context,
                )
            } else {
                KeyBinding::new("enter", Enter, context)
            },
            if composer {
                KeyBinding::new(
                    "shift-enter",
                    gpui_kit::component::input::Enter {
                        secondary: false,
                        shift: true,
                    },
                    context,
                )
            } else {
                KeyBinding::new("shift-enter", SoftBreak, context)
            },
            if composer {
                KeyBinding::new("tab", gpui_kit::component::input::IndentInline, context)
            } else {
                KeyBinding::new("tab", Indent, context)
            },
            if composer {
                KeyBinding::new(
                    "shift-tab",
                    gpui_kit::component::input::OutdentInline,
                    context,
                )
            } else {
                KeyBinding::new("shift-tab", Outdent, context)
            },
        ]);
        #[cfg(target_os = "macos")]
        cx.bind_keys([
            KeyBinding::new("cmd-a", SelectAll, context),
            KeyBinding::new("cmd-c", Copy, context),
            KeyBinding::new("cmd-x", Cut, context),
            KeyBinding::new("cmd-v", Paste, context),
            KeyBinding::new("cmd-z", Undo, context),
            KeyBinding::new("cmd-shift-z", Redo, context),
            KeyBinding::new("cmd-left", Home, context),
            KeyBinding::new("cmd-right", End, context),
            KeyBinding::new("cmd-shift-left", SelectHome, context),
            KeyBinding::new("cmd-shift-right", SelectEnd, context),
            KeyBinding::new("alt-left", WordLeft, context),
            KeyBinding::new("alt-right", WordRight, context),
            KeyBinding::new("alt-shift-left", SelectWordLeft, context),
            KeyBinding::new("alt-shift-right", SelectWordRight, context),
            KeyBinding::new("ctrl-a", Home, context),
            KeyBinding::new("ctrl-e", End, context),
            KeyBinding::new("ctrl-b", Left, context),
            KeyBinding::new("ctrl-f", Right, context),
            if composer {
                KeyBinding::new("ctrl-p", gpui_kit::component::input::MoveUp, context)
            } else {
                KeyBinding::new("ctrl-p", Up, context)
            },
            if composer {
                KeyBinding::new("ctrl-n", gpui_kit::component::input::MoveDown, context)
            } else {
                KeyBinding::new("ctrl-n", Down, context)
            },
            if composer {
                KeyBinding::new("ctrl-h", gpui_kit::component::input::Backspace, context)
            } else {
                KeyBinding::new("ctrl-h", Backspace, context)
            },
            KeyBinding::new("ctrl-d", Delete, context),
        ]);
        #[cfg(not(target_os = "macos"))]
        cx.bind_keys([
            KeyBinding::new("ctrl-a", SelectAll, context),
            KeyBinding::new("ctrl-c", Copy, context),
            KeyBinding::new("ctrl-x", Cut, context),
            KeyBinding::new("ctrl-v", Paste, context),
            KeyBinding::new("ctrl-z", Undo, context),
            KeyBinding::new("ctrl-shift-z", Redo, context),
            KeyBinding::new("ctrl-y", Redo, context),
            KeyBinding::new("ctrl-left", WordLeft, context),
            KeyBinding::new("ctrl-right", WordRight, context),
            KeyBinding::new("ctrl-shift-left", SelectWordLeft, context),
            KeyBinding::new("ctrl-shift-right", SelectWordRight, context),
        ]);
    }
    // Source-based drafts expose selection formatting, not invisible stored
    // marks for future typing. The document editor retains its existing keys.
    for context in [Some(CONTEXT), Some("MarkdownComposer && MarkdownSelection")] {
        #[cfg(target_os = "macos")]
        cx.bind_keys([
            KeyBinding::new("cmd-b", Bold, context),
            KeyBinding::new("cmd-i", Italic, context),
            KeyBinding::new("cmd-e", Code, context),
            KeyBinding::new("cmd-shift-x", Strike, context),
        ]);
        #[cfg(not(target_os = "macos"))]
        cx.bind_keys([
            KeyBinding::new("ctrl-b", Bold, context),
            KeyBinding::new("ctrl-i", Italic, context),
            KeyBinding::new("ctrl-e", Code, context),
            KeyBinding::new("ctrl-shift-x", Strike, context),
        ]);
    }
}
