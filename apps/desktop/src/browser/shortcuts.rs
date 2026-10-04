//! Native WebViews do not participate in GPUI's key dispatch tree.
use super::*;

pub(super) fn init(cx: &mut App) {
    #[cfg(target_os = "macos")]
    if cx
        .key_bindings()
        .borrow()
        .bindings_for_action(&inspector::Inspect)
        .next()
        .is_none()
    {
        cx.bind_keys([KeyBinding::new(
            "secondary-shift-i",
            inspector::Inspect,
            Some("SailryBrowser"),
        )]);
    }
}

impl Browser {
    pub(super) fn sync_shortcuts(&self, cx: &App) {
        let script = script(cx);
        for tab in &self.tabs {
            if let Some(page) = &tab.page
                && let Err(error) = page.read(cx).raw().evaluate_script(&script)
            {
                eprintln!("could not update browser shortcuts: {error}");
            }
        }
    }
}

pub(super) fn script(cx: &App) -> String {
    let mut actions = vec![(
        crate::shell::shortcuts::CloseFocused.boxed_clone(),
        "close-tab",
    )];
    #[cfg(target_os = "macos")]
    actions.push((inspector::Inspect.boxed_clone(), "inspect"));
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let bindings: Vec<_> = actions.into_iter()
        .filter_map(|(action, name)| {
            let binding = keymap.bindings_for_action(action.as_ref()).next()?;
            let strokes: Vec<_> = binding.keystrokes().iter().map(|key| key.as_keystroke()).map(|key| {
                serde_json::json!({"key": key.key, "meta": key.modifiers.platform, "ctrl": key.modifiers.control, "alt": key.modifiers.alt, "shift": key.modifiers.shift})
            }).collect();
            (!strokes.is_empty()).then(|| serde_json::json!({"strokes":strokes,"action":name}))
        }).collect();
    format!(
        "(() => {{ const bindings = {}; {} }})();",
        serde_json::to_string(&bindings).expect("shortcut JSON"),
        include_str!("shortcuts.js")
    )
}
