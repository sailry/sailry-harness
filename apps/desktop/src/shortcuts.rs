//! Configurable application-wide commands. System and component shortcuts keep
//! their fixed defaults outside this registry; plugins own scoped bindings.
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
pub(crate) struct Registry(Vec<Shortcut>);
impl Global for Registry {}

impl Registry {
    pub(crate) fn add<A: Action + Clone>(
        &mut self,
        id: &'static str,
        label: &'static str,
        key: &str,
        contexts: &[Option<&str>],
        action: A,
    ) {
        assert!(
            !self.0.iter().any(|entry| entry.id == id),
            "duplicate shortcut ID"
        );
        self.0.push(Shortcut {
            id,
            label,
            key: key.into(),
            default_key: key.into(),
            bindings: contexts
                .iter()
                .map(|context| KeyBinding::new(key, action.clone(), *context))
                .collect(),
        });
    }
}

pub(crate) fn init(cx: &mut App) {
    if cx.try_global::<Registry>().is_some() {
        return;
    }
    let mut registry = Registry::default();
    registry.add(
        "app.search",
        "search",
        "secondary-k",
        &[None],
        shell::Search,
    );
    registry.add(
        "sidebar.toggle",
        "sidebar_toggle",
        "secondary-b",
        &[None],
        shell::ToggleSidebar,
    );
    registry.add(
        "panel.toggle",
        "shortcut_inspector",
        "secondary-shift-b",
        &[Some("Sailry")],
        shell::ToggleDetails,
    );
    registry.add(
        "host.refresh",
        "shortcut_sync",
        "secondary-r",
        &[Some("Sailry")],
        RefreshHost,
    );
    let overrides = crate::preferences::data(cx).shortcuts.unwrap_or_default();
    for entry in &mut registry.0 {
        if let Some(key) = overrides.get(entry.id)
            && valid(key)
            && !fixed_conflict(key, cx)
        {
            entry.key = key.clone();
        }
    }
    cx.set_global(registry);
    apply(cx);
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
    for entry in &cx.global::<Registry>().0 {
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
                    &DummyKeyboardMapper,
                )
                .expect("validated shortcut")
                .with_meta(OWNED),
            );
        }
    }
    cx.clear_key_bindings();
    cx.bind_keys(bindings);
    #[cfg(target_os = "macos")]
    if cx.get_menus().is_some() {
        // Native menu key equivalents are captured when the menu is installed.
        crate::app_menu::refresh(cx);
    }
    cx.refresh_windows();
}

pub(crate) fn valid(key: &str) -> bool {
    key.is_empty()
        || (key.split_whitespace().count() <= 2
            && key
                .split_whitespace()
                .all(|part| Keystroke::parse(part).is_ok())
            && !key.trim().is_empty())
}

pub(crate) fn entries(cx: &App) -> Vec<Shortcut> {
    cx.try_global::<Registry>()
        .map(|r| r.0.clone())
        .unwrap_or_default()
}

pub(crate) fn save(id: &str, key: Option<&str>, cx: &mut App) -> Result<(), &'static str> {
    let Some(registry) = cx.try_global::<Registry>() else {
        return Err("shortcut_unavailable");
    };
    let Some(entry) = registry.0.iter().find(|entry| entry.id == id) else {
        return Err("shortcut_unavailable");
    };
    let candidate = key.unwrap_or(&entry.default_key).trim().to_lowercase();
    if !valid(&candidate) {
        return Err("shortcut_invalid");
    }
    if fixed_conflict(&candidate, cx) {
        return Err("shortcut_conflict");
    }
    let parsed = candidate
        .split_whitespace()
        .map(Keystroke::parse)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "shortcut_invalid")?;
    if !parsed.is_empty()
        && registry
            .0
            .iter()
            .filter(|other| other.id != id)
            .any(|other| {
                let other_keys = other
                    .key
                    .split_whitespace()
                    .filter_map(|key| Keystroke::parse(key).ok())
                    .collect::<Vec<_>>();
                (parsed.starts_with(&other_keys) || other_keys.starts_with(&parsed))
                    && !other_keys.is_empty()
                    && entry.bindings.iter().any(|a| {
                        other
                            .bindings
                            .iter()
                            .any(|b| match (a.predicate(), b.predicate()) {
                                (Some(a), Some(b)) => a.is_superset(&b) || b.is_superset(&a),
                                _ => true,
                            })
                    })
            })
    {
        return Err("shortcut_conflict");
    }
    let reset = key.is_none();
    cx.update_global::<Registry, _>(|registry, _| {
        registry
            .0
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
        .0
        .iter()
        .find(|entry| entry.id == id)
        .map(|entry| entry.key.clone())
}

fn fixed_conflict(candidate: &str, cx: &App) -> bool {
    let keys = candidate
        .split_whitespace()
        .filter_map(|key| Keystroke::parse(key).ok())
        .collect::<Vec<_>>();
    if keys.is_empty() {
        return false;
    }
    cx.key_bindings()
        .borrow()
        .bindings()
        .filter(|binding| binding.meta() != Some(OWNED))
        .any(|binding| {
            let other = binding
                .keystrokes()
                .iter()
                .map(|key| key.as_keystroke().clone())
                .collect::<Vec<_>>();
            !other.is_empty() && (keys.starts_with(&other) || other.starts_with(&keys))
        })
}

pub(crate) fn label(key: &str) -> String {
    key.split_whitespace()
        .filter_map(|part| Keystroke::parse(part).ok())
        .map(|key| gpui_kit::component::kbd::Kbd::format(&key))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests;
