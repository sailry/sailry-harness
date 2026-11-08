//! Native WebViews do not participate in GPUI's key dispatch tree.
use super::*;

actions!(sailry, [NewBrowserTab, ReloadBrowser]);

pub(crate) fn init(cx: &mut App) {
    #[derive(Default)]
    struct Installed;
    impl Global for Installed {}
    if cx.has_global::<Installed>() {
        return;
    }
    cx.set_global(Installed);
    let actions = [
        ("secondary-t", NewBrowserTab.boxed_clone()),
        ("secondary-r", ReloadBrowser.boxed_clone()),
        ("secondary-shift-c", annotation::SelectElement.boxed_clone()),
        #[cfg(target_os = "macos")]
        ("secondary-shift-i", inspector::Inspect.boxed_clone()),
    ];
    for (key, action) in actions {
        cx.bind_keys([KeyBinding::load(
            key,
            action,
            Some(
                KeyBindingContextPredicate::parse("SailryBrowser")
                    .expect("static shortcut context")
                    .into(),
            ),
            false,
            None,
            cx,
        )
        .expect("static shortcut")]);
    }
}

impl Browser {
    pub(crate) fn new_tab(&mut self, _: &NewBrowserTab, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(state::Shortcut::Add);
    }

    pub(crate) fn reload_page(
        &mut self,
        _: &ReloadBrowser,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _ = self.control(state::Action::Reload {}, window, cx);
    }

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
    let mut actions = vec![
        (
            crate::shell::shortcuts::CloseFocused.boxed_clone(),
            "close-tab",
        ),
        (annotation::SelectElement.boxed_clone(), "select-element"),
        (NewBrowserTab.boxed_clone(), "new-tab"),
        (ReloadBrowser.boxed_clone(), "reload"),
    ];
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
