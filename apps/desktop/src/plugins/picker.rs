//! The pinned script List has no searchable delegate or confirmation/focus exports.
//! Reuse the native Kit list and command frame; packages supply rows and decisions.
use super::{
    host::sdk::values::{decode, encode},
    native_context::Events,
};
use gpui_kit::{
    component::{
        list::{ListDelegate, ListItem, ListState},
        *,
    },
    *,
};
use gpui_shell::{HostError, HostModule};
use sailry_link::CancellationToken;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::Arc};
mod command;

#[derive(Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Item {
    id: String,
    label: String,
    #[serde(default)]
    detail: String,
    icon: Option<String>,
    #[serde(default)]
    group: String,
    #[serde(default)]
    checked: bool,
    #[serde(default)]
    disabled: bool,
    #[serde(default)]
    submenu: bool,
    value: Option<Value>,
}
#[derive(Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Mode {
    #[default]
    List,
    Command,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Props {
    open: bool,
    title: String,
    items: Vec<Item>,
    #[serde(default)]
    loading: bool,
    #[serde(default)]
    empty: String,
    #[serde(default)]
    mode: Mode,
}
struct Rows {
    id: String,
    events: Entity<Events>,
    all: Vec<Item>,
    visible: Vec<Vec<Item>>,
    selected: Option<IndexPath>,
    query: String,
    loading: bool,
    empty: String,
}
impl Rows {
    fn filter(&mut self) {
        let query = self.query.to_lowercase();
        self.visible.clear();
        for row in self.all.iter().filter(|row| {
            format!("{} {}", row.label, row.detail)
                .to_lowercase()
                .contains(&query)
        }) {
            if self
                .visible
                .last()
                .is_none_or(|group| group[0].group != row.group)
            {
                self.visible.push(Vec::new());
            }
            self.visible.last_mut().unwrap().push(row.clone());
        }
        self.selected = self
            .visible
            .iter()
            .enumerate()
            .find_map(|(section, rows)| {
                rows.iter()
                    .position(|row| row.checked && !row.disabled)
                    .map(|row| IndexPath::new(row).section(section))
            })
            .or_else(|| {
                self.visible.iter().enumerate().find_map(|(section, rows)| {
                    rows.iter()
                        .position(|row| !row.disabled)
                        .map(|row| IndexPath::new(row).section(section))
                })
            });
    }
}
impl ListDelegate for Rows {
    type Item = ListItem;
    fn sections_count(&self, _: &App) -> usize {
        self.visible.len().max(1)
    }
    fn items_count(&self, section: usize, _: &App) -> usize {
        self.visible.get(section).map_or(0, Vec::len)
    }
    fn set_selected_index(
        &mut self,
        index: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
        self.selected = index;
    }
    fn perform_search(
        &mut self,
        query: &str,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.query = query.into();
        self.filter();
        cx.notify();
        Task::ready(())
    }
    fn loading(&self, _: &App) -> bool {
        self.loading
    }
    fn render_section_header(
        &mut self,
        section: usize,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<impl IntoElement> {
        let group = &self.visible.get(section)?.first()?.group;
        (!group.is_empty()).then(|| crate::command_picker::heading(group.clone(), cx))
    }
    fn render_item(
        &mut self,
        index: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let row = self.visible.get(index.section)?.get(index.row)?.clone();
        let selector = row.id.clone();
        Some(
            crate::command_picker::row(
                (
                    SharedString::from(self.id.clone()),
                    index.section * 1000 + index.row,
                ),
                row.checked,
                row.disabled,
                cx,
            )
            .on_mouse_enter(cx.listener(move |list, _, window, cx| {
                if list.selected_index() != Some(index) {
                    list.set_selected_index(Some(index), window, cx);
                    cx.notify();
                }
            }))
            .child(
                h_flex()
                    .gap_2()
                    .w_full()
                    .min_w_0()
                    .debug_selector(move || selector.clone())
                    .child(div().w_4().flex_shrink_0().children(row.icon.map(|icon| {
                        Icon::empty()
                            .path(if icon.contains('/') {
                                icon
                            } else {
                                format!("icons/{icon}.svg")
                            })
                            .size_4()
                    })))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_sm()
                            .child(row.label),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .max_w(px(190.))
                            .truncate()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(row.detail),
                    ),
            ),
        )
    }
    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        div()
            .debug_selector(|| "plugin-picker-empty".into())
            .p_3()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(self.empty.clone())
    }
    fn confirm(&mut self, _: bool, _: &mut Window, cx: &mut Context<ListState<Self>>) {
        if let Some(row) = self
            .selected
            .and_then(|index| self.visible.get(index.section)?.get(index.row))
            .filter(|row| !row.disabled)
        {
            self.events
                .read(cx)
                .send(json!({"picker":self.id,"kind":"select","id":row.id,"value":row.value}));
        }
    }
    fn cancel(&mut self, _: &mut Window, cx: &mut Context<ListState<Self>>) {
        self.events
            .read(cx)
            .send(json!({"picker":self.id,"kind":"close"}));
    }
}
struct Retained {
    control: Control,
    previous: Option<ReturnFocus>,
}

#[derive(Clone)]
enum Control {
    List(Entity<ListState<Rows>>),
    Command(Entity<component::command::CommandState>),
}

impl Control {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self {
            Self::List(list) => list.read(cx).focus_handle(cx),
            Self::Command(command) => command.read(cx).focus_handle(cx),
        }
    }

    fn focus(&self, window: &mut Window, cx: &mut App) {
        match self {
            Self::List(list) => list.update(cx, |list, cx| list.focus(window, cx)),
            Self::Command(command) => command.update(cx, |command, cx| command.focus(window, cx)),
        }
    }
}

#[derive(Clone)]
struct ReturnFocus {
    focus: WeakFocusHandle,
    owner: Option<WeakEntity<Retained>>,
    fallback: WeakFocusHandle,
}

type Pickers = Rc<RefCell<BTreeMap<String, WeakEntity<Retained>>>>;

impl Retained {
    fn restore(&mut self, window: &mut Window, cx: &mut App) {
        let Some(previous) = self.previous.take() else {
            return;
        };
        let focus = self.control.focus_handle(cx);
        window.defer(cx, move |window, cx| {
            if !focus.is_focused(window) && window.focused(cx).is_some() {
                return;
            }
            let mounted = previous
                .owner
                .as_ref()
                .is_none_or(|owner| owner.upgrade().is_some());
            if let Some(previous) = mounted
                .then(|| previous.focus.upgrade())
                .flatten()
                .or_else(|| previous.fallback.upgrade())
            {
                previous.focus(window, cx);
            }
        });
    }
}

fn previous(pickers: &Pickers, window: &Window, cx: &App) -> Option<ReturnFocus> {
    let focus = window.focused(cx)?.downgrade();
    let owner = pickers.borrow().values().find_map(|owner| {
        let state = owner.upgrade()?;
        state
            .read(cx)
            .control
            .focus_handle(cx)
            .is_focused(window)
            .then(|| owner.clone())
    });
    let fallback = owner
        .as_ref()
        .and_then(|owner| owner.upgrade())
        .and_then(|owner| {
            owner
                .read(cx)
                .previous
                .as_ref()
                .map(|value| value.fallback.clone())
        })
        .unwrap_or_else(|| focus.clone());
    Some(ReturnFocus {
        focus,
        owner,
        fallback,
    })
}

fn render(
    id: String,
    props: Props,
    events: Entity<Events>,
    pickers: &Pickers,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    if !props.open {
        return div().into_any_element();
    }
    pickers
        .borrow_mut()
        .retain(|_, state| state.upgrade().is_some());
    let mut opened = false;
    // Kit owns this state only while the declaration is rendered. Closed
    // placeholders, changed IDs, and unmounts release its rows and native input.
    let state = window.use_keyed_state(
        SharedString::from(format!(
            "plugin-picker-{}-{id}-{}",
            events.entity_id(),
            if props.mode == Mode::Command {
                "command"
            } else {
                "list"
            }
        )),
        cx,
        |window, cx| {
            opened = true;
            let previous = previous(pickers, window, cx);
            let control = if props.mode == Mode::Command {
                Control::Command(cx.new(|cx| component::command::CommandState::new(window, cx)))
            } else {
                let mut rows = Rows {
                    id: id.clone(),
                    events: events.clone(),
                    all: Vec::new(),
                    visible: Vec::new(),
                    selected: None,
                    query: String::new(),
                    loading: false,
                    empty: String::new(),
                };
                rows.filter();
                Control::List(cx.new(|cx| ListState::new(rows, window, cx).searchable(true)))
            };
            let registry = Rc::downgrade(pickers);
            let released_id = id.clone();
            let entity = cx.entity_id();
            cx.on_release_in(window, move |state: &mut Retained, window, cx| {
                if let Some(registry) = registry.upgrade() {
                    let mut registry = registry.borrow_mut();
                    if registry
                        .get(&released_id)
                        .is_some_and(|state| state.entity_id() == entity)
                    {
                        registry.remove(&released_id);
                    }
                }
                state.restore(window, cx);
            })
            .detach();
            Retained { control, previous }
        },
    );
    pickers.borrow_mut().insert(id.clone(), state.downgrade());
    if opened {
        let owner = state.downgrade();
        window.defer(cx, move |window, cx| {
            if let Some(owner) = owner.upgrade() {
                let control = owner.read(cx).control.clone();
                control.focus(window, cx);
            }
        });
    }
    let control = state.read(cx).control.clone();
    match control {
        Control::List(list) => {
            list.update(cx, |list, cx| {
                let rows = list.delegate_mut();
                let changed = rows.loading != props.loading
                    || rows.empty != props.empty
                    || rows.all != props.items
                    || opened;
                rows.loading = props.loading;
                rows.empty = props.empty;
                if rows.all != props.items || opened {
                    rows.all = props.items;
                    rows.filter();
                    let selected = rows.selected;
                    list.set_selected_index(selected, window, cx);
                }
                if changed {
                    cx.notify();
                }
            });
            popup(id, props.title, list, events, window, cx)
        }
        Control::Command(command) => command::popup(id, props, command, events, window, cx),
    }
}

fn popup(
    id: String,
    title: String,
    list: Entity<ListState<Rows>>,
    events: Entity<Events>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let focus = list.read(cx).focus_handle(cx);
    let width = crate::command_picker::width(window);
    let rows = list.read(cx).delegate();
    let height = crate::command_picker::height(
        rows.visible.iter().map(Vec::len).sum(),
        rows.visible
            .iter()
            .filter(|group| !group[0].group.is_empty())
            .count(),
        window,
    );
    base::Dialog::new(cx)
        .focus_handle(focus)
        .on_ok(|_, _, _| false)
        .on_close({
            let id = id.clone();
            move |_, _, cx| events.read(cx).send(json!({"picker":id,"kind":"close"}))
        })
        .backdrop(div().absolute().inset_0().bg(cx.theme().overlay))
        .popup(
            div()
                .absolute()
                .left((window.viewport_size().width - width) / 2.)
                .top(window.viewport_size().height / 10.)
                .w(width)
                .child(component::surface::render(
                    div()
                        .id(SharedString::from(id.clone()))
                        .debug_selector(move || id.clone())
                        .occlude()
                        .min_h_0()
                        .p_0()
                        .overflow_hidden()
                        .w(width)
                        // Kit's List uses a percentage height. An auto-height popup otherwise
                        // leaves its virtual row viewport at zero after async loading.
                        .h(height)
                        .max_h(window.viewport_size().height * 0.6)
                        .bg(cx.theme().tokens.background)
                        .text_color(cx.theme().foreground)
                        .border_1()
                        .border_color(cx.theme().border)
                        .rounded(cx.theme().surface_radius())
                        .shadow_xl()
                        .child(crate::command_picker::list(&list, title, window)),
                    cx,
                )),
        )
        .into_any_element()
}

pub(super) fn module(module: HostModule, stop: CancellationToken, cx: &mut App) -> HostModule {
    let (sender, receiver) = tokio::sync::mpsc::channel::<Value>(32);
    let events = cx.new(|_| Events::new(sender, stop.clone()));
    let receiver = Arc::new(tokio::sync::Mutex::new(receiver));
    let retained = Rc::new(RefCell::new(BTreeMap::new()));
    let declarations = format!(
        "{}\nexport const Picker: {{new(id:string,props:{{open:boolean;title:string;items:{{id:string;label:string;detail?:string;icon?:string;group?:string;checked?:boolean;disabled?:boolean;submenu?:boolean;value?:unknown}}[];loading?:boolean;empty?:string;mode?:'list'|'command'}}):import(\"gpui-kit\").Element}};\nexport function nextPickerEvent():Promise<{{picker:string;kind:'select'|'close';id?:string;value?:unknown}}>;",
        module.declared().unwrap_or_default()
    );
    module.component("Picker",move|args,window,cx|{
        let props=match decode(args.props()).and_then(|value|serde_json::from_value::<Props>(value).map_err(|error|HostError::new(error.to_string()))) {
            Ok(props)=>props,
            Err(_)=>return div().into_any_element(),
        };
        render(args.id().to_owned(),props,events.clone(),&retained,window,cx)
    }).async_function("nextPickerEvent",move|_|{let receiver=receiver.clone();let stop=stop.clone();Ok(async move {
        let value=tokio::select!{biased;_=stop.cancelled()=>return Err(HostError::new("plugin view is closed")),value=async{receiver.lock().await.recv().await}=>value.ok_or_else(||HostError::new("picker is closed"))?};encode(value)
    })}).declarations(declarations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    mod lifecycle;

    struct Frame {
        list: Entity<ListState<Rows>>,
        events: Entity<Events>,
    }
    impl Render for Frame {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            popup(
                "picker-probe".into(),
                "Search".into(),
                self.list.clone(),
                self.events.clone(),
                window,
                cx,
            )
        }
    }
    fn item(id: &str, group: &str) -> Item {
        Item {
            id: id.into(),
            label: id.into(),
            detail: String::new(),
            icon: None,
            group: group.into(),
            checked: false,
            disabled: false,
            submenu: false,
            value: None,
        }
    }

    #[gpui::test]
    fn renders_loaded_groups_and_confirms_filtered_rows(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
        });
        let (sender, mut receiver) = tokio::sync::mpsc::channel(32);
        let mut list = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let events = cx.new(|_| Events::new(sender, CancellationToken::new()));
            let state = cx.new(|cx| {
                ListState::new(
                    Rows {
                        id: "picker-probe".into(),
                        events: events.clone(),
                        all: Vec::new(),
                        visible: Vec::new(),
                        selected: None,
                        query: String::new(),
                        loading: true,
                        empty: "Empty".into(),
                    },
                    window,
                    cx,
                )
                .searchable(true)
            });
            list = Some(state.clone());
            let frame = cx.new(|cx| {
                cx.observe(&state, |_, _, cx| cx.notify()).detach();
                Frame {
                    list: state,
                    events,
                }
            });
            Root::new(frame, window, cx)
        });
        let list = list.unwrap();
        visual.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
        visual.update(|window, cx| {
            list.update(cx, |list, cx| {
                let rows = list.delegate_mut();
                rows.loading = false;
                rows.all = vec![
                    item("entry-0", "Worktrees"),
                    item("entry-1", "Worktrees"),
                    item("create", "Actions"),
                ];
                rows.filter();
                let selected = rows.selected;
                list.set_selected_index(selected, window, cx);
                list.focus(window, cx);
                cx.notify();
            })
        });
        visual.run_until_parked();
        visual.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
        let viewport = visual.update(|window, _| window.viewport_size());
        let popup = visual
            .debug_bounds("picker-probe")
            .expect("loaded picker must have a surface");
        let width = px(480.).min(viewport.width - px(40.));
        assert_eq!(popup.left(), (viewport.width - width) / 2.);
        assert_eq!(popup.top(), viewport.height / 10.);
        assert_eq!(popup.size.width, width);
        assert!(popup.size.height <= viewport.height * 0.6);
        let search = visual.debug_bounds("list-search").unwrap();
        assert_eq!(search.left() - popup.left(), px(1.));
        assert_eq!(popup.right() - search.right(), px(1.));
        assert_eq!(search.top() - popup.top(), px(1.));
        let bounds = visual
            .debug_bounds("entry-1")
            .expect("loaded second row must have a viewport");
        assert!(bounds.size.height > px(0.));
        assert_eq!(bounds.left() - search.left(), px(16.));
        assert!(visual.debug_bounds("create").is_some());
        visual.simulate_input("entry-1");
        visual.run_until_parked();
        visual
            .executor()
            .advance_clock(std::time::Duration::from_millis(100));
        visual.run_until_parked();
        visual.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
        visual.simulate_keystrokes("enter");
        visual.run_until_parked();
        assert_eq!(
            receiver.try_recv().unwrap(),
            json!({"picker":"picker-probe","kind":"select","id":"entry-1","value":null})
        );
    }
}
