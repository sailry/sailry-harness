//! One pane header composes conversation-scoped entry points.
use super::*;
use crate::plugins::contributions::{Registry, header::Popover};
use sailry_protocol::plugin::ui::Slot;

pub(crate) struct Controls {
    search: Entity<search::Search>,
    assets: Entity<assets::Assets>,
    registry: Entity<Registry>,
    entries: BTreeMap<String, Entity<Popover>>,
    _observer: Subscription,
}

impl Controls {
    pub(super) fn new(
        search: Entity<search::Search>,
        assets: Entity<assets::Assets>,
        registry: Entity<Registry>,
        cx: &mut Context<Self>,
    ) -> Self {
        let observer = cx.observe(&registry, |_, _, cx| cx.notify());
        Self {
            search,
            assets,
            registry,
            entries: BTreeMap::new(),
            _observer: observer,
        }
    }
}

impl Render for Controls {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entries = self.registry.read(cx).entries(Slot::Header, cx);
        let mut visible: Vec<AnyElement> = Vec::new();
        let mut retained = Vec::new();
        for entry in entries {
            if entry.declaration.kind != sailry_protocol::plugin::ui::Kind::Popover {
                visible.push(self.registry.update(cx, |registry, cx| {
                    registry.control(entry, crate::plugins::contributions::Form::Icons, cx)
                }));
                continue;
            }
            let Some(panel) = self.registry.read(cx).popover_panel(&entry, cx) else {
                continue;
            };
            let key = serde_json::to_string(&entry.key).expect("contribution key serializes");
            retained.push(key.clone());
            let control = self.entries.entry(key).or_insert_with(|| {
                cx.new(|cx| Popover::new(entry.clone(), panel.clone(), window, cx))
            });
            control.update(cx, |control, cx| control.update(entry, panel, cx));
            visible.push(control.clone().into_any_element());
        }
        self.entries.retain(|key, _| retained.contains(key));
        h_flex()
            .gap_1()
            .child(self.search.clone())
            .child(self.assets.clone())
            .children(visible)
    }
}
