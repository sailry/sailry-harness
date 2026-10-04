//! Desktop-only workspace membership. Kit owns the layout tree and split geometry.
mod arrange;
mod hierarchy;
mod skin;
mod state;
pub(crate) use state::Saved;
#[cfg(test)]
mod tests;

use gpui_kit::component::dock::AnyDrag;
use gpui_kit::{
    component::{dock::*, *},
    *,
};
use sailry_protocol::{NodeId as HostId, SessionId, TerminalId, WorktreeId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) const LIMIT: usize = 9;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum Target {
    Draft,
    Session(HostId, SessionId),
    Terminal(HostId, WorktreeId, TerminalId),
}

#[derive(Clone)]
pub(crate) struct Drag {
    pub target: Target,
    pub session: Option<sailry_protocol::Session>,
    pub terminal: Option<sailry_protocol::terminal::Info>,
    pub project: Option<sailry_protocol::ProjectId>,
    pub title: SharedString,
}
impl Drag {
    pub fn panel(target: Target, title: SharedString) -> Self {
        Self {
            target,
            title,
            session: None,
            terminal: None,
            project: None,
        }
    }
    pub fn payload(self) -> AnyDrag {
        AnyDrag::new(self)
    }
}
impl Render for Drag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(cx.theme().popover)
            .text_color(cx.theme().popover_foreground)
            .text_sm()
            .child(self.title.clone())
    }
}

pub(crate) enum Event {
    Focus(Target),
    Activate(Target),
    Close(Target),
    RequestClose(Target),
    Drop {
        workspace: u64,
        item: Box<Drag>,
        target: DropTarget,
    },
    Changed,
}

pub(crate) struct Pane {
    target: Target,
    pub(crate) title: SharedString,
    project: SharedString,
    content: AnyView,
    focus: FocusHandle,
    owner: WeakEntity<Workspaces>,
    pub closing: bool,
    pub error: Option<&'static str>,
    pub(crate) extension: Option<Extension>,
    search: Option<AnyView>,
    bounds: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
    _focus: Subscription,
}

/// Renderer provenance is controller state; the resource remains owned by its Node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Extension {
    pub package: String,
}
impl EventEmitter<PanelEvent> for Pane {}
impl Focusable for Pane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl BasePanel for Pane {
    fn panel_name(&self) -> &'static str {
        "sailry-workspace-pane"
    }
    // Closing a terminal is an asynchronous Node command, not a dock removal.
    fn closable(&self, _: &App) -> bool {
        false
    }
    fn zoomable(&self, _: &App) -> bool {
        false
    }
    fn dump(&self, _: &App) -> PanelState {
        PanelState {
            info: PanelInfo::panel(serde_json::to_value(self.target).unwrap()),
            ..PanelState::new(self.panel_name())
        }
    }
}
impl Render for Pane {
    fn render(&mut self, _: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let owner = self.owner.clone();
        let target = self.target;
        let bounds = self.bounds.clone();
        div()
            .id("workspace-pane-content")
            .debug_selector(move || format!("pane-{target:?}"))
            .size_full()
            .min_w_0()
            .min_h_0()
            .on_prepaint(move |value, _, _| bounds.set(value))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let _ = owner.update(cx, |owner, cx| owner.focus(target, cx));
            })
            .child(self.content.clone())
    }
}

struct Workspace {
    area: Entity<DockArea>,
    _events: Subscription,
}

pub(crate) struct Workspaces {
    groups: BTreeMap<u64, Workspace>,
    panes: BTreeMap<Target, Entity<Pane>>,
    hierarchy: hierarchy::Hierarchy,
    pub active: Option<Target>,
    recent: Vec<Target>,
    next: u64,
    save: Option<Task<()>>,
    saving: bool,
}
impl EventEmitter<Event> for Workspaces {}

impl Workspaces {
    pub fn new() -> Self {
        Self {
            groups: BTreeMap::new(),
            panes: BTreeMap::new(),
            hierarchy: hierarchy::Hierarchy::default(),
            active: None,
            recent: Vec::new(),
            next: 0,
            save: None,
            saving: true,
        }
    }
    pub fn controls(&mut self, target: Target, search: AnyView, cx: &mut Context<Self>) {
        if let Some(pane) = self.panes.get(&target) {
            pane.update(cx, |pane, cx| {
                if pane.search.as_ref().map(AnyView::entity_id) != Some(search.entity_id()) {
                    pane.search = Some(search);
                    cx.notify();
                }
            });
        }
    }
    pub fn persist(&mut self, cx: &mut Context<Self>) {
        if !self.saving {
            return;
        }
        self.save = Some(cx.spawn(async move |workspaces, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(350))
                .await;
            let _ = workspaces.update(cx, |workspaces, cx| {
                let saved = serde_json::to_value(workspaces.saved(cx)).unwrap();
                crate::preferences::update(cx, |preferences| preferences.workspaces = Some(saved));
            });
        }));
    }
    pub fn suspend_saving(&mut self) {
        self.save = None;
        self.saving = false;
    }
    pub fn contains(&self, target: Target) -> bool {
        self.panes.contains_key(&target)
    }
    pub fn pane(&self, target: Target) -> Option<&Entity<Pane>> {
        self.panes.get(&target)
    }
    pub fn workspace(&self, target: Target, cx: &App) -> Option<u64> {
        let panel = PanelId::from(self.panes.get(&target)?.entity_id());
        self.groups.iter().find_map(|(id, group)| {
            group
                .area
                .read(cx)
                .layout(DockPlacement::Center)?
                .find_panel_node(panel)
                .map(|_| *id)
        })
    }
    pub fn members(&self, workspace: u64, cx: &App) -> Vec<Target> {
        self.panes
            .keys()
            .copied()
            .filter(|target| self.workspace(*target, cx) == Some(workspace))
            .collect()
    }
    /// The Shell owns the single-pane title band; split groups keep their own headers.
    pub fn single_header(&self, cx: &App) -> Option<(SharedString, AnyElement)> {
        let target = self.active?;
        if self.members(self.workspace(target, cx)?, cx).len() != 1 {
            return None;
        }
        let pane = self.panes.get(&target)?.read(cx);
        Some((pane.title.clone(), pane.actions(false)))
    }

    pub fn is_split(&self, cx: &App) -> bool {
        self.active
            .and_then(|target| self.workspace(target, cx))
            .is_some_and(|workspace| self.members(workspace, cx).len() > 1)
    }

    pub fn active_area(&self, cx: &App) -> Option<Entity<DockArea>> {
        Some(
            self.groups
                .get(&self.workspace(self.active?, cx)?)?
                .area
                .clone(),
        )
    }
    pub fn focus(&mut self, target: Target, cx: &mut Context<Self>) {
        if !self.contains(target) || self.active == Some(target) {
            return;
        }
        if let Some(previous) = self.active {
            self.recent.retain(|entry| *entry != previous);
            self.recent.push(previous);
        }
        self.recent.retain(|entry| *entry != target);
        self.active = Some(target);
        cx.emit(Event::Focus(target));
        cx.emit(Event::Changed);
        cx.notify();
    }
    pub fn open(
        &mut self,
        target: Target,
        title: SharedString,
        project: SharedString,
        content: AnyView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let std::collections::btree_map::Entry::Vacant(e) = self.panes.entry(target) {
            let owner = cx.weak_entity();
            let pane = cx.new(|cx| {
                crate::feedback::observe(window, cx, |pane: &Pane, _| {
                    pane.error.into_iter().collect()
                });
                let focus = cx.focus_handle();
                let _focus = cx.on_focus_in(&focus, window, |pane: &mut Pane, _, cx| {
                    let target = pane.target;
                    let _ = pane.owner.update(cx, |owner, cx| owner.focus(target, cx));
                });
                Pane {
                    target,
                    title,
                    project,
                    content,
                    focus,
                    owner,
                    closing: false,
                    error: None,
                    extension: None,
                    search: None,
                    bounds: Default::default(),
                    _focus,
                }
            });
            e.insert(pane.clone());
            self.standalone(pane, window, cx);
        } else {
            self.hydrate(target, title, project, content, cx);
        }
        self.focus(target, cx);
    }
    fn standalone(&mut self, pane: Entity<Pane>, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.next;
        self.next += 1;
        let area = cx.new(|cx| {
            DockArea::new(format!("workspace-{id}"), Some(1), window, cx)
                .with_renderer(skin::Skin::new(cx))
        });
        area.update(cx, |area, cx| {
            area.set_center(DockLayout::tabs().panel(pane), window, cx)
        });
        let events = cx.subscribe(&area, move |_, _, event, cx| match event {
            DockEvent::DragDrop { item, target } => {
                if let Some(item) = item.value().downcast_ref::<Drag>() {
                    cx.emit(Event::Drop {
                        workspace: id,
                        item: Box::new(item.clone()),
                        target: *target,
                    });
                }
            }
            DockEvent::LayoutChanged => cx.emit(Event::Changed),
        });
        self.groups.insert(
            id,
            Workspace {
                area,
                _events: events,
            },
        );
    }
    pub fn rename(&mut self, old: Target, new: Target, cx: &mut Context<Self>) {
        self.hierarchy.rename(old, new);
        if let Some(pane) = self.panes.remove(&old) {
            pane.update(cx, |pane, cx| {
                pane.target = new;
                cx.notify();
            });
            self.panes.insert(new, pane);
        }
        if self.active == Some(old) {
            self.active = Some(new);
        }
        for target in &mut self.recent {
            if *target == old {
                *target = new;
            }
        }
        cx.emit(Event::Changed);
    }
    pub fn title(&mut self, target: Target, title: SharedString, cx: &mut Context<Self>) {
        if let Some(pane) = self.panes.get(&target) {
            pane.update(cx, |pane, cx| {
                if pane.title != title {
                    pane.title = title;
                    cx.notify();
                }
            });
        }
    }
    pub fn close(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        let Some(group) = self.workspace(target, cx) else {
            return;
        };
        let Some(pane) = self.panes.remove(&target) else {
            return;
        };
        self.recent.retain(|entry| *entry != target);
        self.hierarchy.remove(target);
        self.groups[&group]
            .area
            .update(cx, |area, cx| area.remove_panel(pane, window, cx));
        let next = self.members(group, cx).first().copied();
        self.prune(cx);
        let next = next
            .or_else(|| {
                self.recent
                    .iter()
                    .rev()
                    .copied()
                    .find(|target| self.contains(*target))
            })
            .or_else(|| {
                self.panes
                    .keys()
                    .rev()
                    .copied()
                    .find(|target| *target != Target::Draft)
            });
        if self.active == Some(target) {
            self.active = None;
            if let Some(next) = next {
                self.focus(next, cx);
            }
        }
        cx.emit(Event::Changed);
        cx.notify();
    }
    pub fn parent(&self, target: Target) -> Option<Target> {
        self.hierarchy.0.get(&target).copied()
    }

    pub fn detach(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        let Some(group) = self.workspace(target, cx) else {
            return;
        };
        if self.members(group, cx).len() < 2 {
            return;
        }
        self.saving = true;
        let pane = self.panes[&target].clone();
        self.groups[&group]
            .area
            .update(cx, |area, cx| area.remove_panel(pane.clone(), window, cx));
        self.hierarchy.remove(target);
        self.standalone(pane, window, cx);
        self.prune(cx);
        self.focus(target, cx);
        cx.emit(Event::Changed);
        cx.notify();
    }
    fn prune(&mut self, cx: &App) {
        self.groups
            .retain(|_, group| !group.area.read(cx).is_empty(DockPlacement::Center, cx));
    }
    pub fn can_drop(
        &self,
        workspace: u64,
        target: Target,
        placement: Option<gpui_kit::base::Placement>,
        cx: &App,
    ) -> bool {
        self.groups.contains_key(&workspace)
            && (placement.is_none()
                || self.workspace(target, cx) == Some(workspace)
                || self.members(workspace, cx).len() < LIMIT)
    }
    pub fn drop(
        &mut self,
        workspace: u64,
        target: Target,
        destination: DropTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let node = destination.node();
        let placement = destination.placement();
        if !self.can_drop(workspace, target, placement, cx) {
            return false;
        }
        let Some(source) = self.workspace(target, cx) else {
            return false;
        };
        let area = self.groups[&workspace].area.clone();
        let displaced = {
            let area = area.read(cx);
            let Some(tree) = area.layout(DockPlacement::Center) else {
                return false;
            };
            let Some(node) = tree.find_node(node) else {
                return false;
            };
            let PaneRef::Tabs { panels, .. } = node.kind() else {
                return false;
            };
            let Some(id) = panels.first() else {
                return false;
            };
            self.panes
                .iter()
                .find_map(|(key, pane)| (PanelId::from(pane.entity_id()) == *id).then_some(*key))
        };
        let Some(displaced) = displaced else {
            return false;
        };
        if displaced == target {
            return false;
        }
        // An explicit layout edit may replace an unreadable saved layout; startup may not.
        self.saving = true;
        let pane = self.panes[&target].clone();
        if let Some(placement) = placement {
            self.hierarchy.attach(target, displaced);
            if source != workspace {
                self.groups[&source]
                    .area
                    .update(cx, |area, cx| area.remove_panel(pane.clone(), window, cx));
                area.update(cx, |area, cx| {
                    area.add_panel(pane.clone(), DockPlacement::Center, None, window, cx)
                });
            }
            area.update(cx, |area, cx| {
                area.move_panel(
                    PanelId::from(pane.entity_id()),
                    InsertTarget::Split {
                        node,
                        placement,
                        size: None,
                    },
                    window,
                    cx,
                )
            });
        } else {
            // Transform only Kit's current measured snapshot; never retain a second layout tree.
            let mut layout = area.read(cx).dump(cx).center;
            state::replace(&mut layout, displaced, target);
            if source == workspace {
                self.hierarchy.swap(target, displaced);
                // Replacement used a simultaneous mapping, so the original source becomes the destination.
                layout = state::swap(&area.read(cx).dump(cx).center, target, displaced);
            } else {
                self.hierarchy.remove(target);
                self.hierarchy.rename(displaced, target);
                self.groups[&source]
                    .area
                    .update(cx, |area, cx| area.remove_panel(pane.clone(), window, cx));
                self.standalone(self.panes[&displaced].clone(), window, cx);
            }
            let layout = state::layout(&layout, &self.panes).unwrap();
            area.update(cx, |area, cx| area.set_center(layout, window, cx));
        }
        self.prune(cx);
        self.focus(target, cx);
        cx.emit(Event::Changed);
        cx.notify();
        true
    }
}
impl Render for Workspaces {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(workspace) = self.active.and_then(|target| self.workspace(target, cx)) {
            let members = self.members(workspace, cx);
            let split = members.len() > 1;
            for target in members {
                let content = self.panes[&target].read(cx).content.clone();
                if let Ok(view) = content.downcast::<crate::conversation::live::View>() {
                    view.update(cx, |view, cx| view.set_split(split, cx));
                }
            }
        }
        div()
            .id("split-workspaces")
            .debug_selector(|| "split-workspaces".into())
            .size_full()
            .min_w_0()
            .min_h_0()
            .children(self.active_area(cx))
    }
}
