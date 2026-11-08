//! Configurable application and system commands. Components and plugins own scoped bindings.
use crate::shell::{self, shortcuts::*};
use gpui_kit::*;

#[derive(Clone)]
pub(crate) struct Shortcut {
    pub id: &'static str,
    pub label: &'static str,
    pub key: String,
    bindings: Vec<KeyBinding>,
    default_key: String,
}
#[derive(Default)]
pub(crate) struct Registry {
    entries: Vec<Shortcut>,
    suppressed: Vec<(usize, KeyBinding)>,
}
impl Global for Registry {}

impl Registry {
    pub(crate) fn add<A: Action + Clone>(
        &mut self,
        id: &'static str,
        label: &'static str,
        key: &str,
        contexts: &[Option<&str>],
        action: A,
        cx: &App,
    ) {
        assert!(
            !self.entries.iter().any(|entry| entry.id == id),
            "duplicate shortcut ID"
        );
        self.entries.push(Shortcut {
            id,
            label,
            key: key.into(),
            default_key: key.into(),
            bindings: contexts
                .iter()
                .map(|context| {
                    KeyBinding::load(
                        key,
                        Box::new(action.clone()),
                        context.map(|context| {
                            KeyBindingContextPredicate::parse(context)
                                .expect("static shortcut context")
                                .into()
                        }),
                        false,
                        None,
                        cx,
                    )
                    .expect("static shortcut")
                })
                .collect(),
        });
    }
}

pub(crate) fn init(cx: &mut App) {
    if cx.try_global::<Registry>().is_some() {
        return;
    }
    let mut registry = Registry::default();
    let context = application_context(cx.platform_info().os);
    registry.add(
        "conversation.new",
        "menu_new_conversation",
        "secondary-n",
        &[Some(context)],
        NewConversation,
        cx,
    );
    registry.add(
        "project.new",
        "menu_new_project",
        "secondary-alt-n",
        &[Some("Sailry")],
        NewProject,
        cx,
    );
    registry.add(
        "folder.open",
        "menu_open_folder",
        "secondary-o",
        &[Some(context)],
        OpenFolder,
        cx,
    );
    registry.add(
        "app.search",
        "search",
        "secondary-k",
        &[Some(context)],
        shell::Search,
        cx,
    );
    registry.add(
        "sidebar.toggle",
        "sidebar_toggle",
        "secondary-b",
        &[Some(context)],
        shell::ToggleSidebar,
        cx,
    );
    registry.add(
        "panel.toggle",
        "shortcut_inspector",
        "secondary-alt-b",
        &[Some("Sailry")],
        shell::ToggleDetails,
        cx,
    );
    registry.add(
        "host.refresh",
        "shortcut_sync",
        "secondary-shift-r",
        &[Some("Sailry")],
        RefreshHost,
        cx,
    );
    registry.add(
        "conversation.previous",
        "shortcut_previous_conversation",
        "secondary-shift-[",
        &[Some("Sailry")],
        PreviousConversation,
        cx,
    );
    registry.add(
        "conversation.next",
        "shortcut_next_conversation",
        "secondary-shift-]",
        &[Some("Sailry")],
        NextConversation,
        cx,
    );
    registry.add(
        "app.shortcuts",
        "settings_shortcuts",
        "secondary-/",
        &[Some(context)],
        crate::settings::OpenSection(crate::settings::Section::Shortcuts),
        cx,
    );
    registry.add(
        "system.activate",
        "shortcut_activate",
        "ctrl-alt-space",
        &[],
        shell::Search,
        cx,
    );
    registry.add(
        "system.selection",
        "shortcut_selection",
        "",
        &[],
        shell::Search,
        cx,
    );
    let overrides = crate::preferences::data(cx).shortcuts.unwrap_or_default();
    let mut overridden = std::collections::BTreeSet::new();
    for entry in &mut registry.entries {
        if let Some(key) = overrides.get(entry.id)
            && valid(key, cx.platform_info().os)
        {
            entry.key = key.clone();
            overridden.insert(entry.id);
        }
    }
    for index in 0..registry.entries.len() {
        let entry = &registry.entries[index];
        if !overridden.contains(entry.id)
            && registry.entries.iter().any(|other| {
                overridden.contains(other.id)
                    && overlaps(entry, other)
                    && conflicts(&entry.key, &other.key, cx.platform_info().os)
            })
        {
            registry.entries[index].key.clear();
        }
    }
    cx.set_global(registry);
    apply(cx);
}

fn application_context(os: OperatingSystem) -> &'static str {
    if os == OperatingSystem::Mac {
        "Sailry"
    } else {
        "Sailry && !SailryTerminal"
    }
}

// Kit and plugin bindings remain in their original order. Only this registry's
// tagged bindings are replaced, avoiding accumulating stale overrides.
const OWNED: KeyBindingMetaIndex = KeyBindingMetaIndex(0x5341_494c);
fn apply(cx: &mut App) {
    let mut bindings: Vec<_> = cx
        .key_bindings()
        .borrow()
        .bindings()
        .filter(|binding| binding.meta() != Some(OWNED))
        .cloned()
        .collect();
    let suppressed =
        cx.update_global::<Registry, _>(|registry, _| std::mem::take(&mut registry.suppressed));
    for (index, binding) in suppressed {
        bindings.insert(index.min(bindings.len()), binding);
    }
    let mut suppressed = Vec::new();
    let mut retained = Vec::new();
    for (index, binding) in bindings.into_iter().enumerate() {
        if binding.meta().is_none()
            && cx.global::<Registry>().entries.iter().any(|entry| {
                entry.key != entry.default_key
                    && fixed_binding_conflict(entry.id, &entry.key, &binding, cx.platform_info().os)
            })
        {
            suppressed.push((index, binding));
        } else {
            retained.push(binding);
        }
    }
    let mut bindings = retained;
    for entry in &cx.global::<Registry>().entries {
        if entry.key.is_empty() {
            continue;
        }
        for binding in &entry.bindings {
            bindings.push(
                KeyBinding::load(
                    &entry.key,
                    binding.action().boxed_clone(),
                    binding.predicate(),
                    false,
                    None,
                    cx,
                )
                .expect("validated shortcut")
                .with_meta(OWNED),
            );
        }
    }
    cx.update_global::<Registry, _>(|registry, _| registry.suppressed = suppressed);
    cx.clear_key_bindings();
    cx.bind_keys(bindings);
    #[cfg(target_os = "macos")]
    if cx.get_menus().is_some() {
        // Native menu key equivalents are captured when the menu is installed.
        crate::app_menu::refresh(cx);
    }
    cx.refresh_windows();
}

pub(crate) fn valid(key: &str, os: OperatingSystem) -> bool {
    key.is_empty()
        || (key.split_whitespace().count() <= 2
            && key
                .split_whitespace()
                .all(|part| Keystroke::parse_with_os(part, os).is_ok())
            && !key.trim().is_empty())
}

pub(crate) fn entries(cx: &App) -> Vec<Shortcut> {
    cx.try_global::<Registry>()
        .map(|registry| registry.entries.clone())
        .unwrap_or_default()
}

pub(crate) fn save(id: &str, key: Option<&str>, cx: &mut App) -> Result<(), &'static str> {
    let Some(registry) = cx.try_global::<Registry>() else {
        return Err("shortcut_unavailable");
    };
    let Some(entry) = registry.entries.iter().find(|entry| entry.id == id) else {
        return Err("shortcut_unavailable");
    };
    let candidate = key.unwrap_or(&entry.default_key).trim().to_lowercase();
    if !valid(&candidate, cx.platform_info().os) {
        return Err("shortcut_invalid");
    }
    if candidate != entry.key && fixed_conflict(id, &candidate, cx) {
        return Err("shortcut_conflict");
    }
    if registry.entries.iter().any(|other| {
        other.id != id
            && overlaps(entry, other)
            && conflicts(&candidate, &other.key, cx.platform_info().os)
    }) {
        return Err("shortcut_conflict");
    }
    let reset = key.is_none();
    if id.starts_with("system.") {
        crate::system_shortcuts::configure(id, &candidate, cx)?;
    }
    cx.update_global::<Registry, _>(|registry, _| {
        registry
            .entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .unwrap()
            .key = candidate.clone();
    });
    crate::preferences::update(cx, |data| {
        let overrides = data.shortcuts.get_or_insert_with(Default::default);
        if reset {
            overrides.remove(id);
        } else {
            overrides.insert(id.to_owned(), candidate);
        }
    });
    apply(cx);
    Ok(())
}

#[cfg(test)]
pub(crate) fn key(id: &str, cx: &App) -> Option<String> {
    cx.try_global::<Registry>()?
        .entries
        .iter()
        .find(|entry| entry.id == id)
        .map(|entry| entry.key.clone())
}

fn fixed_conflict(id: &str, candidate: &str, cx: &App) -> bool {
    cx.key_bindings()
        .borrow()
        .bindings()
        .filter(|binding| binding.meta() != Some(OWNED))
        .chain(
            cx.try_global::<Registry>()
                .into_iter()
                .flat_map(|registry| registry.suppressed.iter().map(|(_, binding)| binding)),
        )
        .any(|binding| fixed_binding_conflict(id, candidate, binding, cx.platform_info().os))
}

fn fixed_binding_conflict(
    id: &str,
    candidate: &str,
    binding: &KeyBinding,
    os: OperatingSystem,
) -> bool {
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = id;
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    if id == "host.refresh"
        && binding
            .action()
            .as_any()
            .is::<crate::browser::shortcuts::ReloadBrowser>()
    {
        return false;
    }
    binding_conflict(candidate, binding, os)
}

pub(crate) fn renderer_conflict(candidate: &str, cx: &App) -> bool {
    let browser = [KeyContext::parse("SailryBrowser").expect("static shortcut context")];
    !valid(candidate, cx.platform_info().os)
        || entries(cx)
            .iter()
            .any(|entry| conflicts(candidate, &entry.key, cx.platform_info().os))
        || ["escape", "up", "shift-tab", "secondary-enter"]
            .iter()
            .any(|key| conflicts(candidate, key, cx.platform_info().os))
        || cx.key_bindings().borrow().bindings().any(|binding| {
            binding.meta().is_none()
                && binding.action().name().starts_with("sailry::")
                && !binding
                    .predicate()
                    .is_some_and(|predicate| predicate.eval(&browser))
                && binding_conflict(candidate, binding, cx.platform_info().os)
        })
}

fn binding_conflict(candidate: &str, binding: &KeyBinding, os: OperatingSystem) -> bool {
    let keys = candidate
        .split_whitespace()
        .map(|key| Keystroke::parse_with_os(key, os))
        .collect::<Result<Vec<_>, _>>();
    let Ok(keys) = keys else {
        return false;
    };
    let other = binding
        .keystrokes()
        .iter()
        .map(|key| key.as_keystroke().clone())
        .collect::<Vec<_>>();
    !keys.is_empty() && !other.is_empty() && (keys.starts_with(&other) || other.starts_with(&keys))
}

fn overlaps(left: &Shortcut, right: &Shortcut) -> bool {
    left.bindings.is_empty()
        || right.bindings.is_empty()
        || left.bindings.iter().any(|left| {
            right
                .bindings
                .iter()
                .any(|right| match (left.predicate(), right.predicate()) {
                    (Some(left), Some(right)) => {
                        left.is_superset(&right) || right.is_superset(&left)
                    }
                    _ => true,
                })
        })
}

fn conflicts(left: &str, right: &str, os: OperatingSystem) -> bool {
    let parse = |key: &str| {
        key.split_whitespace()
            .map(|key| Keystroke::parse_with_os(key, os))
            .collect::<Result<Vec<_>, _>>()
    };
    let (Ok(left), Ok(right)) = (parse(left), parse(right)) else {
        return false;
    };
    !left.is_empty() && !right.is_empty() && (left.starts_with(&right) || right.starts_with(&left))
}

pub(crate) fn label(key: &str, os: OperatingSystem) -> String {
    key.split_whitespace()
        .filter_map(|part| Keystroke::parse_with_os(part, os).ok())
        .map(|key| gpui_kit::component::kbd::Kbd::format(&key))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests;
