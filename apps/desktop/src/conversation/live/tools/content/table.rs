use super::*;
use gpui_kit::component::{
    table::{Column, DataTable, TableDelegate, TableState},
    tooltip::Tooltip,
};

#[derive(IntoElement)]
pub(super) struct Table {
    pub id: SharedString,
    pub data: sailry_protocol::tool::Table,
}

struct Rows {
    id: SharedString,
    data: sailry_protocol::tool::Table,
}

impl TableDelegate for Rows {
    fn columns_count(&self, _: &App) -> usize {
        self.data.columns.len()
    }
    fn rows_count(&self, _: &App) -> usize {
        self.data.rows.len()
    }
    fn column(&self, index: usize, _: &App) -> Column {
        Column::new(format!("column-{index}"), self.data.columns[index].clone())
            .width(px(180.))
            .movable(false)
    }
    fn render_td(
        &mut self,
        row: usize,
        column: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let text = self.data.rows[row][column].clone();
        let selector = format!("{}-cell-{row}-{column}", self.id);
        div()
            .id(SharedString::from(selector.clone()))
            .size_full()
            .text_sm()
            .truncate()
            .debug_selector(move || selector.clone())
            .child(text.clone())
            .tooltip(move |window, cx| Tooltip::new(text.clone()).build(window, cx))
    }
    fn cell_text(&self, row: usize, column: usize, _: &App) -> String {
        self.data.rows[row][column].clone()
    }
}

impl RenderOnce for Table {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let table = window.use_keyed_state(self.id.clone(), cx, |window, cx| {
            TableState::new(
                Rows {
                    id: self.id.clone(),
                    data: self.data.clone(),
                },
                window,
                cx,
            )
            .col_movable(false)
            .sortable(false)
        });
        if table.read(cx).delegate().data != self.data {
            table.update(cx, |table, cx| {
                table.delegate_mut().data = self.data.clone();
                table.refresh(cx);
            });
        }
        div()
            .id(self.id.clone())
            .debug_selector(move || self.id.to_string())
            .w_full()
            .min_w_0()
            .h(rems((self.data.rows.len().clamp(1, 5) + 1) as f32 * 1.75))
            .child(DataTable::new(&table).small().bordered(false))
    }
}
