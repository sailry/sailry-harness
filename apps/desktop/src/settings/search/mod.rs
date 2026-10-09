mod editor;
mod requests;
#[cfg(test)]
mod tests;
mod view;

use super::{Workspace, providers::Binding};
use gpui_kit::{
    component::{
        searchable_list::SearchableListItem,
        select::{SelectEvent, SelectState},
        *,
    },
    *,
};
use sailry_protocol::{
    NodeId,
    search::{Driver, Selection, Settings, Update},
};
use std::collections::BTreeMap;

pub(super) struct State {
    preview: Entity<Panel>,
    panels: BTreeMap<NodeId, Entity<Panel>>,
    selected: Option<NodeId>,
}

pub(super) struct Panel {
    binding: Option<Binding>,
    active: bool,
    saved: Option<Settings>,
    picker: Entity<SelectState<Vec<Choice>>>,
    observed: Option<u64>,
    pending: bool,
    error: Option<&'static str>,
}

#[derive(Clone)]
struct Choice(Option<Selection>);

impl SearchableListItem for Choice {
    type Value = Option<Selection>;
    fn title(&self) -> SharedString {
        crate::tr(match self.0 {
            None => "search_driver_none",
            Some(Selection::Auto) => "search_driver_auto",
            Some(Selection::Driver(driver)) => label(driver),
        })
    }
    fn value(&self) -> &Self::Value {
        &self.0
    }
}

fn label(driver: Driver) -> &'static str {
    match driver {
        Driver::Exa => "search_driver_exa",
        Driver::Tavily => "search_driver_tavily",
        Driver::Brave => "search_driver_brave",
        Driver::Searxng => "search_driver_searxng",
        Driver::Bocha => "search_driver_bocha",
    }
}

fn id(driver: Driver) -> &'static str {
    match driver {
        Driver::Exa => "exa",
        Driver::Tavily => "tavily",
        Driver::Brave => "brave",
        Driver::Searxng => "searxng",
        Driver::Bocha => "bocha",
    }
}

impl State {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Workspace>) -> Self {
        Self {
            preview: cx.new(|cx| Panel::new(None, window, cx)),
            panels: Default::default(),
            selected: None,
        }
    }
}

impl Panel {
    fn new(binding: Option<Binding>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        crate::feedback::observe_with(
            window,
            cx,
            |panel: &Self, _| panel.error.into_iter().collect(),
            |_, key, cx| {
                let owner = cx.weak_entity();
                notification::Notification::error(crate::tr(key)).action(move |_, _, cx| {
                    let owner = owner.clone();
                    button::Button::new("search-reload")
                        .label(crate::tr("search_reload"))
                        .on_click(cx.listener(move |toast, _, window, cx| {
                            toast.dismiss(window, cx);
                            _ = owner.update(cx, |panel, cx| panel.load(window, cx));
                        }))
                })
            },
        );
        let mut choices = vec![
            Choice(Some(Selection::Auto)),
            Choice(Some(Selection::Driver(Driver::Exa))),
        ];
        choices.extend(
            Driver::ALL
                .into_iter()
                .filter(|driver| *driver != Driver::Exa)
                .map(|driver| Choice(Some(Selection::Driver(driver)))),
        );
        choices.push(Choice(None));
        let picker = cx.new(|cx| SelectState::new(choices, Some(IndexPath::new(0)), window, cx));
        cx.subscribe_in(&picker, window, |panel, picker, event, window, cx| {
            if matches!(event, SelectEvent::Confirm(_)) && !panel.locked() {
                let driver = picker
                    .read(cx)
                    .selected_value()
                    .cloned()
                    .unwrap_or_default();
                if let Some(mut settings) = panel.saved.clone()
                    && settings.driver != driver
                {
                    settings.driver = driver;
                    if let Some(Selection::Driver(driver)) = driver {
                        settings.providers.entry(driver).or_default().enabled = true;
                    }
                    panel.save(settings, window, cx);
                }
            }
        })
        .detach();
        Self {
            saved: binding.is_none().then(Settings::default),
            binding,
            active: false,
            picker,
            observed: None,
            pending: false,
            error: None,
        }
    }

    fn locked(&self) -> bool {
        !self.active
            || self.pending
            || self.saved.is_none()
            || self.error == Some("search_save_unknown")
    }

    fn apply(&mut self, settings: Settings, window: &mut Window, cx: &mut Context<Self>) {
        self.picker.update(cx, |picker, cx| {
            picker.set_selected_value(&settings.driver, window, cx)
        });
        self.saved = Some(settings);
        cx.notify();
    }

    fn accept(&mut self, revision: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.observed == Some(revision) {
            return;
        }
        self.observed = Some(revision);
        if self
            .saved
            .as_ref()
            .is_none_or(|settings| settings.revision != revision)
        {
            self.load(window, cx);
        }
    }

    fn save(&mut self, settings: Settings, window: &mut Window, cx: &mut Context<Self>) {
        self.submit(
            sailry_protocol::Command::SaveSearchSettings(Update {
                settings,
                credentials: Default::default(),
            }),
            window,
            cx,
        );
    }
}

impl Workspace {
    pub(in crate::settings) fn clear_search(&mut self, cx: &mut Context<Self>) {
        if let Some(node) = self.search.selected.take()
            && let Some(panel) = self.search.panels.get(&node)
        {
            panel.update(cx, |panel, cx| {
                panel.active = false;
                cx.notify();
            });
        }
        self.search
            .preview
            .update(cx, |panel, _| panel.active = false);
    }

    pub(super) fn search(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let panel = if let Some(live) = &self.provider_link {
            let binding = live.binding.clone();
            let node = binding.client.target();
            if self.search.selected != Some(node) {
                if let Some(previous) = self.search.selected
                    && let Some(panel) = self.search.panels.get(&previous)
                {
                    panel.update(cx, |panel, _| panel.active = false);
                }
                self.search.selected = Some(node);
            }
            let panel = self
                .search
                .panels
                .entry(binding.client.target())
                .or_insert_with(|| cx.new(|cx| Panel::new(Some(binding), window, cx)))
                .clone();
            if live.connected {
                panel.update(cx, |panel, cx| {
                    panel.active = true;
                    panel.accept(live.search_revision, window, cx)
                });
            }
            panel
        } else {
            self.search
                .preview
                .update(cx, |panel, _| panel.active = true);
            self.search.preview.clone()
        };
        panel.into_any_element()
    }
}
