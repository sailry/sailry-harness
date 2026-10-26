//! The pinned script List has no searchable delegate or confirmation/focus exports.
//! Reuse the native Kit list and command frame; packages supply rows and decisions.
use super::{
    host::sdk::values::{decode, encode},
    native_context::Events,
};
use crate::ui::skeleton::block;
use gpui_kit::prelude::FluentBuilder as _;
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
    mode: Mode,
}
impl Rows {
    fn enabled(&self, _: &App) -> Vec<IndexPath> {
        self.visible
            .iter()
            .enumerate()
            .flat_map(|(section, rows)| {
                rows.iter()
                    .enumerate()
                    .filter(|(_, row)| !row.disabled)
                    .map(move |(row, _)| IndexPath::new(row).section(section))
            })
            .collect()
    }

    fn height(&self) -> f32 {
        if self.mode == Mode::Command && self.all.iter().any(|row| !row.detail.is_empty()) {
            48.
        } else {
            crate::command_picker::ROW_HEIGHT
        }
    }

    fn filter(&mut self) {
        let query = self.query.to_lowercase();
        self.visible.clear();
        for row in self.all.iter().filter(|row| {
            format!("{} {}", row.label, row.detail)
                .to_lowercase()
                .contains(&query)
        }) {
            if self.visible.is_empty()
                || (self.mode == Mode::List
                    && self
                        .visible
                        .last()
                        .is_some_and(|group| group[0].group != row.group))
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
                    .position(|row| self.mode == Mode::List && row.checked && !row.disabled)
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
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.query = query.into();
        self.filter();
        let selected = self.selected;
        let owner = cx.weak_entity();
        window.defer(cx, move |window, cx| {
            let _ = owner.update(cx, |list, cx| {
                list.set_selected_index(selected, window, cx);
                cx.notify();
            });
        });
        cx.notify();
        Task::ready(())
    }
    fn loading(&self, _: &App) -> bool {
        self.loading
    }
    fn render_loading(
        &mut self,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        // The loading body has the same extent as one command row.
        div().p_2().child(
            h_flex()
                .h(px(crate::command_picker::ROW_HEIGHT))
                .px_2()
                .child(block(cx).h_3().w_48().max_w_full()),
        )
    }
    fn render_section_header(
        &mut self,
        section: usize,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<impl IntoElement> {
        let group = &self.visible.get(section)?.first()?.group;
        (self.mode == Mode::List && !group.is_empty())
            .then(|| crate::command_picker::heading(group.clone(), cx))
    }
    fn render_item(
        &mut self,
        index: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let row = self.visible.get(index.section)?.get(index.row)?.clone();
        let selector = row.id.clone();
        let item = crate::command_picker::row(
            (
                SharedString::from(self.id.clone()),
                index.section * 1000 + index.row,
            ),
            row.checked,
            row.disabled,
            cx,
        )
        .when(!row.disabled, |item| {
            item.on_mouse_enter(cx.listener(move |list, _, window, cx| {
                if list.selected_index() != Some(index) {
                    list.set_selected_index(Some(index), window, cx);
                    cx.notify();
                }
            }))
        });
        if self.mode == Mode::Command {
            return Some(command::row(item, row, self.height(), cx));
        }
        Some(
            item.child(
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
}
struct Retained {
    list: Entity<ListState<Rows>>,
    _enter: Subscription,
    previous: Option<ReturnFocus>,
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
        let focus = self.list.read(cx).focus_handle(cx);
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
            .list
            .read(cx)
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
            let mut rows = Rows {
                id: id.clone(),
                events: events.clone(),
                all: Vec::new(),
                visible: Vec::new(),
                selected: None,
                query: String::new(),
                loading: false,
                empty: String::new(),
                mode: props.mode,
            };
            rows.filter();
            let list = cx.new(|cx| ListState::new(rows, window, cx).searchable(true));
            let enter = crate::command_picker::intercept(&list, Rows::enabled, cx);
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
            Retained {
                list,
                previous,
                _enter: enter,
            }
        },
    );
    pickers.borrow_mut().insert(id.clone(), state.downgrade());
    if opened {
        let owner = state.downgrade();
        window.defer(cx, move |window, cx| {
            if let Some(owner) = owner.upgrade() {
                let list = owner.read(cx).list.clone();
                list.update(cx, |list, cx| list.focus(window, cx));
            }
        });
    }
    let list = state.read(cx).list.clone();
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
    let height = if rows.loading {
        crate::command_picker::height(1, 0, window)
    } else {
        crate::command_picker::extent(
            rows.visible.iter().map(Vec::len).sum(),
            rows.visible
                .iter()
                .filter(|group| rows.mode == Mode::List && !group[0].group.is_empty())
                .count(),
            rows.height(),
            window,
        )
    };
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
                .child(
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
                        .bg(cx.theme().popover)
                        .text_color(cx.theme().popover_foreground)
                        .border_1()
                        .border_color(cx.theme().border)
                        .rounded(cx.theme().radius_lg)
                        .shadow_xl()
                        .child(crate::command_picker::list(&list, title, window)),
                ),
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
