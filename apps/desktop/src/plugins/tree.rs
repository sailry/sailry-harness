//! Kit's script Tree has no row events, multi-selection, native menus or drag hooks.
//! Retain its native TreeState and compose the existing resource row adapter.
pub(in crate::plugins) mod drops;
mod row;
use super::{
    host::sdk::values::{decode, encode},
    native_context::{self, Events},
};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::{
    component::{
        tree::{TreeEntry, TreeEvent, TreeItem, TreeState},
        *,
    },
    *,
};
use gpui_shell::{HostError, HostModule};
use sailry_link::CancellationToken;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::Arc};

use crate::resources::{
    CopySelection as Copy, CutSelection as Cut, ExtendFilesDown as ExtendDown,
    ExtendFilesUp as ExtendUp, PasteSelection as Paste, RenameSelection as Rename,
    SelectFiles as SelectAll,
};

#[derive(Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Item {
    id: String,
    label: String,
    icon: Option<String>,
    decoration: Option<row::Decoration>,
    #[serde(default)]
    expanded: bool,
    #[serde(default)]
    disabled: bool,
    #[serde(default)]
    children: Vec<Item>,
    #[serde(default)]
    menu: Vec<native_context::Item>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Props {
    items: Vec<Item>,
    #[serde(default)]
    variant: Variant,
    #[serde(default)]
    external_files: bool,
    indent: Option<f32>,
    skip_depth: Option<usize>,
    #[serde(default)]
    selected: Vec<String>,
    current: Option<String>,
    #[serde(default)]
    menu: Vec<native_context::Item>,
}
#[derive(Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Variant {
    #[default]
    Resource,
    Branch,
}
impl Item {
    fn native(&self) -> TreeItem {
        TreeItem::new(self.id.clone(), self.label.clone())
            .expanded(self.expanded)
            .disabled(self.disabled)
            .children(self.children.iter().map(Self::native))
    }
    fn collect(&self, map: &mut BTreeMap<String, Item>) {
        map.insert(self.id.clone(), self.clone());
        for child in &self.children {
            child.collect(map);
        }
    }
}
struct Retained {
    tree: Entity<TreeState>,
    items: Vec<Item>,
    cursor: Option<String>,
    _subscriptions: Vec<Subscription>,
}
impl Retained {
    fn new(
        id: String,
        events: Entity<Events>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let tree = cx.new(|cx| TreeState::new(cx));
        let expanded = cx.subscribe(&tree, {
            let id = id.clone();
            let events = events.clone();
            move |_, _, event: &TreeEvent, cx| {
                let (kind, path) = match event {
                    TreeEvent::Expanded(path) => ("expand", path),
                    TreeEvent::Collapsed(path) => ("collapse", path),
                };
                events
                    .read(cx)
                    .send(json!({"tree":id,"id":path.as_ref(),"kind":kind}));
            }
        });
        let selected = cx.observe_in(&tree, window, move |retained,tree,window,cx| {
            let current=tree.read(cx).selected_item().map(|item|item.id.to_string());
            if retained.cursor != current {
                retained.cursor=current.clone();
                if let Some(current)=current { let modifiers=window.modifiers(); events.read(cx).send(json!({"tree":id,"id":current,"kind":"select","shift":modifiers.shift,"additive":modifiers.platform||modifiers.control})); }
            }
        });
        Self {
            tree,
            items: Vec::new(),
            cursor: None,
            _subscriptions: vec![expanded, selected],
        }
    }
}
#[derive(Clone)]
struct Drag {
    events: Entity<Events>,
    tree: String,
    id: String,
    label: String,
    icon: String,
}
impl Render for Drag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_2()
            .px_3()
            .py_2()
            .rounded(cx.theme().radius)
            .bg(cx.theme().popover)
            .text_color(cx.theme().popover_foreground)
            .text_sm()
            .child(icon(&self.icon))
            .child(self.label.clone())
    }
}
fn icon(value: &str) -> Icon {
    Icon::empty()
        .path(if value.contains('/') {
            value.to_owned()
        } else {
            format!("icons/{value}.svg")
        })
        .size_4()
}

pub(super) fn module(
    module: HostModule,
    stop: CancellationToken,
    drops: drops::Drops,
    cx: &mut App,
) -> HostModule {
    let (sender, receiver) = tokio::sync::mpsc::channel::<Value>(128);
    let events = cx.new(|_| Events::new(sender, stop.clone()));
    let receiver = Arc::new(tokio::sync::Mutex::new(receiver));
    let trees = Rc::new(RefCell::new(BTreeMap::<String, Entity<Retained>>::new()));
    let declarations = format!(
        "{}\n{}",
        module.declared().unwrap_or_default(),
        include_str!("tree.d.ts")
    );
    module.component("ResourceTree",move |args,window,cx| {
        let Ok(props)=decode(args.props()).and_then(|value|serde_json::from_value::<Props>(value).map_err(|error|HostError::new(error.to_string()))) else {return div().into_any_element();};
        let id=args.id().to_owned();
        let retained=trees.borrow_mut().entry(id.clone()).or_insert_with(||cx.new(|cx|Retained::new(id.clone(),events.clone(),window,cx))).clone();
        retained.update(cx,|retained,cx| {
            if retained.items != props.items {
                retained.items=props.items.clone();
                retained.cursor=props.current.clone();
                retained.tree.update(cx,|tree,cx| {tree.set_items(props.items.iter().map(Item::native).collect::<Vec<_>>(),cx); tree.set_selected_index(props.current.as_ref().and_then(|id|tree.index_of(&id.clone().into())),cx);});
            } else if retained.cursor != props.current {
                retained.cursor=props.current.clone();
                retained.tree.update(cx,|tree,cx|tree.set_selected_index(props.current.as_ref().and_then(|id|tree.index_of(&id.clone().into())),cx));
            }
        });
        let tree=retained.read(cx).tree.clone();
        let mut map=BTreeMap::new(); for item in &props.items {item.collect(&mut map);}
        let last=row::last_children(&props.items); let branch=props.variant==Variant::Branch;
        let row_drops=drops.clone(); let external_files=props.external_files;
        let root_drops=drops.clone(); let root_drop_events=events.clone(); let root_drop_id=id.clone();
        let row_events=events.clone(); let row_id=id.clone(); let row_tree=tree.clone();
        let indent=props.indent.unwrap_or(16.); let skip_depth=props.skip_depth.unwrap_or(0);
        let native=gpui_kit::base::Tree::new(&tree).item(move |ix,entry,state,window,cx| {
            let Some(item)=map.get(entry.item().id.as_ref()) else {return div().into_any_element();};
            let item=item.clone(); let path=item.id.clone(); let folder=entry.is_folder();
            let selected=props.selected.contains(&path);
            let glyph=item.icon.clone().unwrap_or_else(||if folder {"folder".into()} else {"file".into()});
            let left=row_events.clone(); let left_id=row_id.clone(); let left_path=path.clone(); let owner=retained.clone(); let left_tree=row_tree.clone();
            let context=row_events.clone(); let context_id=row_id.clone(); let context_path=path.clone(); let menu=item.menu.clone();
            let click=row_events.clone(); let click_id=row_id.clone(); let click_path=path.clone();
            let drop_events=row_events.clone(); let drop_id=row_id.clone(); let drop_path=path.clone();
            let drag_events=row_events.clone(); let drag_tree=row_id.clone(); let drag_path=path.clone();
            let disabled=item.disabled;
            let external_drop=row_drops.clone(); let external_events=row_events.clone(); let external_tree=row_id.clone(); let external_target=path.clone();
            let row=if branch {
                h_flex().id(ix).w_full().min_w_0().child(row::branch(ix,entry,state.is_selected(),last.contains(&path),cx))
            } else {
                crate::resources::rows::row(ix,selected,cx)
                .h_7().text_sm().px_2().pl(px(indent)*entry.depth().saturating_sub(skip_depth)+px(8.))
                .child(row::content(&item,&glyph,row_events.clone(),row_id.clone(),window,cx))
            };
            row.debug_selector({let selector=item.decoration.as_ref().and_then(|row|row.selector.clone()).unwrap_or_else(||format!("resource-file-{path}")); move ||selector.clone()})
                .tooltip({let label=item.label.clone();move |window,cx|tooltip::Tooltip::new(label.clone()).build(window,cx)})
                .on_mouse_down(MouseButton::Left,move |event,window,cx| {
                    if disabled {cx.stop_propagation();return;}
                    owner.update(cx,|owner,_|owner.cursor=Some(left_path.clone()));
                    left_tree.update(cx,|tree,cx|{tree.set_selected_index(Some(ix),cx);tree.focus(window,cx);});
                    left.read(cx).send(json!({"tree":left_id,"id":left_path,"kind":"select","shift":event.modifiers.shift,"additive":event.modifiers.platform||event.modifiers.control}));
                    cx.stop_propagation();
                })
                .on_mouse_down(MouseButton::Right,move |event,window,cx| {
                    if !disabled {context.read(cx).send(json!({"tree":context_id,"id":context_path,"kind":"context"}));native_context::show(&menu,context.clone(),json!({"tree":context_id,"id":context_path,"kind":"menu"}),"action",event.position,window,cx);}
                    cx.stop_propagation();
                })
                .on_click(move |_,window,cx| {let modifiers=window.modifiers(); if disabled||modifiers.shift||modifiers.platform||modifiers.control{return;}
                    if folder {window.dispatch_action(Box::new(gpui_kit::base::actions::Confirm{secondary:false}),cx);} else {click.read(cx).send(json!({"tree":click_id,"id":click_path,"kind":"open"}));}})
                .on_drag(Drag{events:row_events.clone(),tree:row_id.clone(),id:path,label:item.label,icon:glyph},|drag,_,_,cx|cx.new(|_|drag.clone()))
                .can_drop(move |value,_,_| !disabled && ((external_files&&value.is::<ExternalPaths>()) || value.downcast_ref::<Drag>().is_some_and(|drag|drag.events==drag_events&&drag.tree==drag_tree&&drag.id!=drag_path)))
                .on_drop(move |drag:&Drag,_,cx|drop_events.read(cx).send(json!({"tree":drop_id,"id":drop_path,"kind":"drop","source":drag.id})))
                .on_drop(move |paths:&ExternalPaths,_,cx| { if external_files&&!disabled {external_drop.publish(paths,external_events.read(cx),&external_tree,&external_target);cx.stop_propagation();} })
                .into_any_element()
        }).list_style(StyleRefinement::default().flex_grow_1().size_full()).size_full();
        let command=|action:&'static str| {let events=events.clone();let id=id.clone();let tree=tree.clone();move |_:&dyn Action,_:&mut Window,cx:&mut App|{let path=tree.read(cx).selected_item().map(|item|item.id.to_string()).unwrap_or_default();events.read(cx).send(json!({"tree":id,"id":path,"kind":"command","action":action}));}};
        let all=command("select_all");let rename=command("rename");let copy=command("copy");let cut=command("cut");let paste=command("paste");
        let keyboard=events.clone();let keyboard_id=id.clone();let keyboard_tree=tree.clone();
        let root=events.clone();let root_id=id.clone();
        div().id(SharedString::from(id.clone())).debug_selector({let id=id.clone();move ||id.clone()}).key_context("FileTree").size_full()
            .capture_action(move |_:&gpui_kit::base::actions::Confirm,_,cx| {if let Some(entry)=keyboard_tree.read(cx).selected_entry() {if entry.is_folder(){cx.propagate();} else {keyboard.read(cx).send(json!({"tree":keyboard_id,"id":entry.item().id.as_ref(),"kind":"open"}));}}})
            .on_action(move |action:&SelectAll,window,cx|all(action,window,cx))
            .on_action(move |action:&Rename,window,cx|rename(action,window,cx))
            .on_action(move |action:&Copy,window,cx|copy(action,window,cx))
            .on_action(move |action:&Cut,window,cx|cut(action,window,cx))
            .on_action(move |action:&Paste,window,cx|paste(action,window,cx))
            .on_action({let command=command("select_all");move |action:&gpui_kit::component::input::SelectAll,window,cx|command(action,window,cx)})
            .on_action({let command=command("copy");move |action:&gpui_kit::component::input::Copy,window,cx|command(action,window,cx)})
            .on_action({let command=command("cut");move |action:&gpui_kit::component::input::Cut,window,cx|command(action,window,cx)})
            .on_action({let command=command("paste");move |action:&gpui_kit::component::input::Paste,window,cx|command(action,window,cx)})
            .on_action({let tree=tree.clone();move |_:&ExtendUp,_,cx|extend(&tree,false,cx)})
            .on_action({let tree=tree.clone();move |_:&ExtendDown,_,cx|extend(&tree,true,cx)})
            .on_mouse_down(MouseButton::Right,move |event,window,cx|{native_context::show(&props.menu,root.clone(),json!({"tree":root_id,"id":"","kind":"menu"}),"action",event.position,window,cx);cx.stop_propagation();})
            .can_drop(move |value,_,_|external_files&&value.is::<ExternalPaths>())
            .on_drop(move |paths:&ExternalPaths,_,cx|{if external_files {root_drops.publish(paths,root_drop_events.read(cx),&root_drop_id,"");cx.stop_propagation();}})
            .child(native).vertical_scrollbar(tree.read(cx).scroll_handle()).into_any_element()
    }).async_function("nextTreeEvent",move |_| {let receiver=receiver.clone();let stop=stop.clone();Ok(async move {let value=tokio::select!{biased;_=stop.cancelled()=>return Err(HostError::new("plugin view is closed")),value=async{receiver.lock().await.recv().await}=>value.ok_or_else(||HostError::new("tree is closed"))?};encode(value)})}).declarations(declarations)
}
fn extend(tree: &Entity<TreeState>, down: bool, cx: &mut App) {
    tree.update(cx, |tree, cx| {
        let current = tree.selected_index().unwrap_or(0);
        let next = if down && tree.entry(current + 1).is_some() {
            current + 1
        } else if !down {
            current.saturating_sub(1)
        } else {
            current
        };
        tree.set_selected_index(Some(next), cx);
    });
}
