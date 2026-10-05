//! Kit's script Table lacks retained multi-selection and native row menus.
//! Compose its TableState delegate; package code owns row actions and SQL policy.
use super::{
    host::sdk::values::{decode, encode},
    native_context::{self, Events},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    component::{
        Size,
        table::{Column, DataTable, TableDelegate, TableState},
        tooltip::Tooltip,
        *,
    },
    *,
};
use gpui_shell::{HostError, HostModule};
use sailry_link::CancellationToken;
use serde::Deserialize;
use serde_json::json;
use std::{collections::BTreeSet, sync::Arc};
mod cells;

#[derive(Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Props {
    revision: String,
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
    #[serde(default)]
    cells: Vec<Vec<cells::Cell>>,
    row_height: Option<f32>,
    #[serde(default)]
    page: usize,
    #[serde(default)]
    menu: Vec<native_context::Item>,
    #[serde(default)]
    empty: String,
    #[serde(default)]
    empty_icon: Option<String>,
    #[serde(default)]
    widths: Vec<f32>,
    #[serde(default)]
    alignments: Vec<Alignment>,
    #[serde(default)]
    details: Vec<Vec<String>>,
    #[serde(default)]
    row_ids: Vec<String>,
    #[serde(default = "resizable")]
    resizable: bool,
    #[serde(default)]
    stripe: bool,
    #[serde(default)]
    empty_stripes: bool,
    #[serde(default = "resizable")]
    header: bool,
}
fn resizable() -> bool {
    true
}
#[derive(Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Alignment {
    Start,
    End,
}
struct Rows {
    id: String,
    props: Props,
    selected: BTreeSet<usize>,
    anchor: Option<usize>,
    cursor: Option<usize>,
    events: Entity<Events>,
}
impl Rows {
    fn size(&self) -> Size {
        self.props
            .row_height
            .map_or(Size::Medium, |height| Size::Size(px(height)))
    }
    fn cell(&self, column: usize) -> Div {
        div()
            .size_full()
            .text_align(match self.props.alignments.get(column) {
                Some(Alignment::End) => TextAlign::Right,
                _ => TextAlign::Left,
            })
    }
    fn count(&self) -> usize {
        self.props
            .rows
            .len()
            .saturating_sub(self.props.page * 50)
            .min(50)
    }
    fn select(&mut self, row: usize, modifiers: Modifiers) {
        if modifiers.shift {
            let anchor = *self.anchor.get_or_insert(row);
            if !modifiers.platform && !modifiers.control {
                self.selected.clear();
            }
            self.selected.extend(anchor.min(row)..=anchor.max(row));
        } else if modifiers.platform || modifiers.control {
            if !self.selected.remove(&row) {
                self.selected.insert(row);
            }
            self.anchor = Some(row);
        } else {
            self.selected = BTreeSet::from([row]);
            self.anchor = Some(row);
        }
        self.cursor = Some(row);
    }
    fn event(&self, kind: &str, row: Option<usize>, column: Option<usize>) -> serde_json::Value {
        json!({"table":self.id,"revision":self.props.revision,"kind":kind,"row":row.map(|row|row+self.props.page*50),"column":column,"rows":self.selected.iter().map(|row|row+self.props.page*50).collect::<Vec<_>>()})
    }
    fn menu(
        &mut self,
        row: usize,
        column: Option<usize>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        if !self.selected.contains(&row) {
            self.select(row, Modifiers::default());
        }
        native_context::show(
            &self.props.menu,
            self.events.clone(),
            self.event("action", Some(row), column),
            "action",
            position,
            window,
            cx,
        );
        cx.notify();
    }
}
impl TableDelegate for Rows {
    fn columns_count(&self, _: &App) -> usize {
        self.props.columns.len()
    }
    fn rows_count(&self, _: &App) -> usize {
        self.count()
    }
    fn column(&self, index: usize, _: &App) -> Column {
        Column::new(format!("column-{index}"), self.props.columns[index].clone())
            .width(px(self.props.widths.get(index).copied().unwrap_or(180.)))
            .resizable(self.props.resizable)
            .movable(false)
            .when(
                self.size() == Size::Medium
                    && self
                        .props
                        .cells
                        .iter()
                        .any(|row| row.get(index).is_some_and(|cell| cell.secondary.is_some())),
                |column| {
                    // Two text lines retain native medium row height; Kit's
                    // default vertical inset is intended for scalar cells.
                    column.paddings(gpui_kit::Edges {
                        top: px(0.),
                        bottom: px(0.),
                        ..Size::Medium.table_cell_padding()
                    })
                },
            )
    }
    fn render_header(
        &mut self,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let id = self.id.clone();
        div()
            .id("header")
            .debug_selector(move || format!("{id}-header"))
            .when(!self.props.header, |header| header.h_0().overflow_hidden())
    }
    fn render_th(
        &mut self,
        column: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        // The pinned Kit table does not apply Column.align to its header content.
        let id = format!("{}-header-{column}", self.id);
        self.cell(column).v_flex().justify_center().child(
            div()
                .debug_selector(move || id.clone())
                .w_full()
                .child(self.props.columns[column].clone()),
        )
    }
    fn render_tr(
        &mut self,
        row: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        // Kit calls the delegate for viewport filler rows as well as data rows.
        // Those presentation rows must not select data or open a native menu.
        if row >= self.count() {
            let id = self.id.clone();
            return div()
                .id(("data-filler", row))
                .debug_selector(move || format!("{id}-filler-{row}"));
        }
        div()
            .id(("data-row", row))
            .debug_selector({
                let id = self.id.clone();
                move || format!("{id}-row-{row}")
            })
            .when(self.selected.contains(&row), |row| {
                row.bg(crate::theme::sidebar_item(true, false, cx))
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |table, event: &MouseDownEvent, window, cx| {
                    table.delegate_mut().select(row, event.modifiers);
                    table.focus_handle(cx).focus(window, cx);
                    table
                        .delegate()
                        .events
                        .read(cx)
                        .send(table.delegate().event("selection", Some(row), None));
                    cx.notify();
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |table, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    table
                        .delegate_mut()
                        .menu(row, None, event.position, window, cx);
                }),
            )
    }
    fn render_td(
        &mut self,
        row: usize,
        column: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let value = self.props.rows[row + self.props.page * 50][column].clone();
        let detail = self
            .props
            .details
            .get(row + self.props.page * 50)
            .and_then(|cells| cells.get(column))
            .cloned()
            .unwrap_or_else(|| value.clone());
        let cell = self
            .props
            .cells
            .get(row + self.props.page * 50)
            .and_then(|cells| cells.get(column));
        let content = cell.map(|cell| {
            cell.render(
                &format!("{}-cell-{row}-{column}", self.id),
                self.props
                    .alignments
                    .get(column)
                    .copied()
                    .unwrap_or(Alignment::Start),
                self.size() == Size::Medium,
                cx,
            )
        });
        self.cell(column)
            .id(("data-cell", row * self.props.columns.len() + column))
            .debug_selector({
                let id = self.id.clone();
                move || format!("{id}-cell-{row}-{column}")
            })
            .text_sm()
            .when_some(content, |cell, content| cell.child(content))
            .when(cell.is_none(), |cell| cell.truncate().child(value.clone()))
            .tooltip(move |window, cx| Tooltip::new(detail.clone()).build(window, cx))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |table, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    table
                        .delegate_mut()
                        .menu(row, Some(column), event.position, window, cx);
                }),
            )
    }
    fn cell_text(&self, row: usize, column: usize, _: &App) -> String {
        self.props.rows[row + self.props.page * 50][column].clone()
    }
    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        if let Some(icon) = &self.props.empty_icon {
            let icon = Icon::empty().path(if icon.contains('/') {
                icon.clone()
            } else {
                format!("icons/{icon}.svg")
            });
            return crate::empty_state::list_content(
                icon,
                self.props.empty.clone().into(),
                format!("{}-empty", self.id).into(),
                cx,
            )
            .h_full()
            .min_h_0();
        }
        div()
            .p_4()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(self.props.empty.clone())
    }
    fn render_last_empty_col(
        &mut self,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        div()
    }
}

pub(super) fn module(module: HostModule, stop: CancellationToken, cx: &mut App) -> HostModule {
    let (sender, receiver) = tokio::sync::mpsc::channel(64);
    let events = cx.new(|_| Events::new(sender, stop.clone()));
    let receiver = Arc::new(tokio::sync::Mutex::new(receiver));
    let declarations = format!(
        "{}\n{}",
        module.declared().unwrap_or_default(),
        include_str!("data_table.d.ts")
    );
    module.component("DataTable",move|args,window,cx| {
        let Ok(props)=decode(args.props()).and_then(|value|serde_json::from_value::<Props>(value).map_err(|error|HostError::new(error.to_string()))) else{return div().into_any_element();};
        if props.rows.iter().any(|row|row.len()!=props.columns.len())
            || !props.cells.is_empty() && (props.cells.len()!=props.rows.len() || props.cells.iter().any(|row|row.len()!=props.columns.len()))
            || props.row_height.is_some_and(|height|!height.is_finite() || !(32. ..=96.).contains(&height))
            || props.widths.iter().any(|width| !width.is_finite() || *width < 16. || *width > 4096.)
            || !props.details.is_empty() && (props.details.len()!=props.rows.len() || props.details.iter().any(|row|row.len()!=props.columns.len()))
            || !props.row_ids.is_empty() && (props.row_ids.len()!=props.rows.len() || props.row_ids.iter().collect::<BTreeSet<_>>().len()!=props.rows.len())
        {return div().into_any_element();}
        let id=args.id().to_owned();
        let table=window.use_keyed_state(SharedString::from(format!("plugin-table-{id}")),cx,|window,cx|TableState::new(Rows{id:id.clone(),props:props.clone(),selected:Default::default(),anchor:None,cursor:None,events:events.clone()},window,cx).col_movable(false).sortable(false));
        if table.read(cx).delegate().props != props {
            table.update(cx,|table,cx| {
                let rows=table.delegate_mut();
                if rows.props.revision!=props.revision || rows.props.page!=props.page || rows.props.rows!=props.rows {
                    let selected: BTreeSet<_> = rows.selected.iter().filter_map(|index| rows.props.row_ids.get(index + rows.props.page * 50)).collect();
                    let retained: BTreeSet<_> = props.row_ids.iter().skip(props.page * 50).take(50).enumerate()
                        .filter_map(|(index,id)| selected.contains(id).then_some(index)).collect();
                    rows.selected=retained;rows.anchor=rows.selected.first().copied();rows.cursor=rows.anchor;
                }
                rows.props=props;table.refresh(cx);
            });
        }
        let stripe=table.read(cx).delegate().props.stripe;
        let empty_stripes=table.read(cx).delegate().props.empty_stripes;
        let header=table.read(cx).delegate().props.header;
        let size=table.read(cx).delegate().size();
        let owner=table.clone();
        div().id(SharedString::from(id.clone())).debug_selector(move||id.clone()).size_full().min_h_0().min_w_0()
            .on_key_down(move|event:&KeyDownEvent,window,cx|owner.update(cx,|table,cx| {
                let rows=table.delegate_mut();let count=rows.count();if count==0{return;}
                let key=event.keystroke.key.as_str();let modifiers=event.keystroke.modifiers;
                let command=if cfg!(target_os="macos"){modifiers.platform}else{modifiers.control};
                if command && key=="a" {rows.selected=(0..count).collect();}
                else if command && key=="c" {rows.events.read(cx).send(rows.event("copy",rows.selected.first().copied(),None));}
                else if matches!(key,"up"|"down"|"home"|"end") {
                    let target=match(key,rows.cursor){("home",_)=>0,("end",_)=>count-1,("up",Some(row))=>row.saturating_sub(1),("down",Some(row))=>(row+1).min(count-1),_=>0};
                    rows.select(target,modifiers);table.scroll_to_row(target,cx);
                }else{return;}
                table.focus_handle(cx).focus(window,cx);cx.stop_propagation();cx.notify();
            })).child(DataTable::new(&table).with_size(Size::Medium).when(size!=Size::Medium, |table|table.with_size(size)).bordered(false).stripe(stripe).empty_stripes(empty_stripes).scrollbar_visible(header,header)).into_any_element()
    }).async_function("nextTableEvent",move |_|{let receiver=receiver.clone();let stop=stop.clone();Ok(async move {
        let value=tokio::select!{biased;_ = stop.cancelled()=>return Err(HostError::new("plugin view is closed")),value=async{receiver.lock().await.recv().await}=>value.ok_or_else(||HostError::new("table is closed"))?};encode(value)
    })}).declarations(declarations)
}

#[cfg(test)]
mod tests;
