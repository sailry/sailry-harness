//! Core and feature controls are providers of the same declared product slots.
use super::*;
use crate::plugins::contributions::{Entry, Form};
use sailry_protocol::plugin::ui::{self, Align, Contribution, Slot};
mod native;
#[cfg(test)]
mod tests;

struct Native {
    declaration: Contribution,
    toolbar: fn(&View, &mut Context<View>) -> Option<AnyElement>,
    menu: fn(&View, &mut Context<View>) -> Option<AnyElement>,
}

enum Provider {
    Native(Box<Native>),
    External(Box<Entry>),
}

impl View {
    pub(crate) fn renderer_shortcuts(
        &self,
        cx: &App,
    ) -> Vec<(sailry_protocol::plugin::desktop::ResourceKind, String)> {
        if !self.node.connected {
            return Vec::new();
        }
        self.contributions.read(cx).renderer_shortcuts(cx)
    }

    #[cfg(test)]
    pub(crate) fn plugin_panel(
        &self,
        name: &str,
        cx: &App,
    ) -> Option<Entity<crate::plugins::Panel>> {
        self.contributions.read(cx).test_panel(name)
    }

    #[cfg(test)]
    pub(crate) fn contribution_enabled(
        &self,
        package: &str,
        id: &str,
        slot: Slot,
        cx: &App,
    ) -> bool {
        self.contributions
            .read(cx)
            .entries(slot, cx)
            .iter()
            .any(|entry| {
                entry.key.package.name == package && entry.key.id == id && entry.state.enabled
            })
    }

    pub(super) fn registered_statistics(&self, cx: &App) -> Vec<super::super::usage::Group> {
        let mut groups = Vec::new();
        let mut packages: BTreeMap<String, Vec<Entry>> = BTreeMap::new();
        for entry in self.contributions.read(cx).entries(Slot::Statistics, cx) {
            packages
                .entry(entry.key.package.name.clone())
                .or_default()
                .push(entry);
        }
        groups.extend(packages.into_values().map(super::super::usage::external));
        groups.sort_by_key(|group| {
            group
                .metrics
                .iter()
                .map(|metric| metric.order)
                .min()
                .unwrap_or_default()
        });
        groups
    }

    pub(crate) fn contribution_intent(
        &self,
        intent: impl AsRef<str>,
        value: serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<serde_json::Value, String>> {
        crate::plugins::contributions::intents::request(
            self.contributions.clone(),
            intent,
            Some(value),
            window,
            cx,
        )
    }

    pub(crate) fn query_contribution(
        &self,
        intent: ui::Intent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<serde_json::Value, String>> {
        crate::plugins::contributions::intents::request(
            self.contributions.clone(),
            intent,
            None,
            window,
            cx,
        )
    }

    pub(crate) fn can_invoke_contribution(&self, intent: ui::Intent, cx: &App) -> bool {
        self.contributions.read(cx).intent(intent, cx).is_some()
    }

    #[cfg(test)]
    pub(crate) fn renderer_available(
        &self,
        resource: sailry_protocol::plugin::desktop::ResourceKind,
        cx: &App,
    ) -> bool {
        self.contributions.read(cx).renderer_available(resource, cx)
    }

    pub(crate) fn renderer_unavailable(
        &self,
        resource: sailry_protocol::plugin::desktop::ResourceKind,
        cx: &App,
    ) -> bool {
        self.contributions
            .read(cx)
            .renderer_unavailable(resource, cx)
    }

    pub(super) fn registered_controls(
        &self,
        slot: Slot,
        align: Align,
        form: Form,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut entries = Vec::new();
        for native in native::controls() {
            if native.declaration.slot == slot && native.declaration.align == align {
                entries.push((
                    native.declaration.order,
                    native.declaration.id.clone(),
                    Provider::Native(Box::new(native)),
                ));
            }
        }
        for entry in self.contributions.read(cx).entries(slot, cx) {
            if entry.declaration.align == align
                // The location chooser already owns these two worktree actions.
                && !location_action(&entry.declaration)
                && match form {
                    Form::Menu => {
                        entry.declaration.kind != ui::Kind::Indicator
                            && (slot == Slot::Composer
                                || self.sidebar
                                || entry.declaration.overflow == ui::Overflow::Menu)
                    }
                    Form::Toolbar | Form::Icons => {
                        !self.compact_composer || entry.declaration.overflow != ui::Overflow::Menu
                    }
                    Form::Project => false,
                }
            {
                entries.push((
                    entry.declaration.order,
                    format!("{}:{}", entry.key.package.name, entry.key.id),
                    Provider::External(Box::new(entry)),
                ));
            }
        }
        entries.sort_by(|left, right| (&left.0, &left.1).cmp(&(&right.0, &right.1)));
        entries
            .into_iter()
            .filter_map(|(_, _, provider)| match provider {
                Provider::Native(native) => {
                    if form == Form::Menu {
                        (native.menu)(self, cx)
                    } else {
                        (native.toolbar)(self, cx)
                    }
                }
                Provider::External(entry) => {
                    let form = if form == Form::Toolbar
                        && entry.declaration.overflow == ui::Overflow::Menu
                    {
                        Form::Icons
                    } else {
                        form
                    };
                    Some(
                        self.contributions
                            .update(cx, |registry, cx| registry.control(*entry, form, cx)),
                    )
                }
            })
            .collect()
    }
}

fn location_action(entry: &Contribution) -> bool {
    [ui::Intent::CreateWorktree, ui::Intent::ForkWorktree]
        .into_iter()
        .any(|intent| entry.intent.as_deref() == Some(intent.as_ref()))
}
