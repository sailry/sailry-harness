//! Measured Kit rows retain the existing whole-page scroll and two-column cards.
use super::*;
use crate::settings::resource_card;
use gpui_kit::component::scroll::{Scrollbar, ScrollbarMode};
use std::collections::BTreeMap;

type Text = (String, String, bool);

struct Row {
    names: Vec<String>,
    text: Vec<Text>,
    focus: FocusHandle,
}

pub(super) struct Inventory {
    pub(super) list: gpui_kit::ListState,
    rows: Vec<Row>,
    metrics: Option<(Pixels, TextStyle, String)>,
}

impl Inventory {
    fn new() -> Self {
        Self {
            // Measure the finite inventory once for an exact scrollbar extent;
            // ordinary scrolling renders only visible rows and focused controls.
            list: gpui_kit::ListState::new(0, ListAlignment::Top, px(100.)).measure_all(),
            rows: Vec::new(),
            metrics: None,
        }
    }

    fn sync(&mut self, content: Vec<(Vec<String>, Vec<Text>)>, window: &Window, cx: &mut App) {
        let metrics = (
            window.rem_size(),
            window.text_style().clone(),
            rust_i18n::locale().to_string(),
        );
        if self.metrics.as_ref() != Some(&metrics) {
            self.list.remeasure();
            self.metrics = Some(metrics);
        }
        if self
            .rows
            .iter()
            .map(|row| &row.names)
            .ne(content.iter().map(|row| &row.0))
        {
            let anchor = self.list.logical_scroll_top();
            let anchored = self.rows.get(anchor.item_ix).map(|row| row.names.clone());
            let old = std::mem::take(&mut self.rows)
                .into_iter()
                .map(|row| (row.names.clone(), row))
                .collect::<BTreeMap<_, _>>();
            self.rows = content
                .into_iter()
                .map(|(names, text)| Row {
                    focus: old
                        .get(&names)
                        .map(|row| row.focus.clone())
                        .unwrap_or_else(|| cx.focus_handle()),
                    names,
                    text,
                })
                .collect();
            self.list.splice_focusable(
                0..self.list.item_count(),
                self.rows.iter().map(|row| Some(row.focus.clone())),
            );
            if let Some(index) =
                anchored.and_then(|names| self.rows.iter().position(|row| row.names == names))
            {
                self.list.scroll_to(ListOffset {
                    item_ix: index,
                    offset_in_item: anchor.offset_in_item,
                });
            }
        } else {
            for (index, (row, (_, text))) in self.rows.iter_mut().zip(content).enumerate() {
                if row.text != text {
                    row.text = text;
                    self.list.remeasure_items(index..index + 1);
                }
            }
        }
    }
}

impl Workspace {
    pub(in crate::settings) fn inventory(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let packages = self.installed_plugins(cx);
        let catalog = &self.plugin_catalog;
        let updates = catalog.updates.as_ref().map(|state| state.read(cx));
        let metadata = catalog.metadata.as_ref().map(|state| state.read(cx));
        let mut heading = vec![
            (
                tr("settings_plugins").to_string(),
                tr("settings_plugins_description").to_string(),
                catalog.retryable(cx),
            ),
            (
                String::new(),
                String::new(),
                updates.is_some_and(|state| {
                    packages
                        .iter()
                        .any(|plugin| state.has_candidate(&plugin.name))
                        || state.applying()
                }),
            ),
            (
                String::new(),
                String::new(),
                packages.is_empty() && self.provider_link.as_ref().unwrap().connected,
            ),
            (String::new(), String::new(), catalog.python.connected),
            (
                String::new(),
                String::new(),
                metadata.is_some_and(|metadata| metadata.settled()),
            ),
            (
                String::new(),
                String::new(),
                metadata.is_some_and(|metadata| !metadata.errors.is_empty()),
            ),
        ];
        if catalog.python.connected {
            heading.extend(
                catalog
                    .python
                    .preparations
                    .iter()
                    .filter(|preparation| {
                        !catalog
                            .packages
                            .iter()
                            .any(|plugin| plugin.name == preparation.package)
                    })
                    .map(|preparation| {
                        (
                            preparation.package.clone(),
                            preparation::label(preparation.stage).to_string(),
                            false,
                        )
                    }),
            );
        }
        let mut content = vec![(Vec::new(), heading)];
        content.extend(packages.chunks(2).map(|plugins| {
            let names = plugins.iter().map(|plugin| plugin.name.clone()).collect();
            let text = plugins
                .iter()
                .map(|plugin| {
                    let (title, description) = self.caption(plugin, cx);
                    let available = updates
                        .and_then(|state| state.entries.get(&plugin.name))
                        .is_some_and(|entry| entry.candidate.is_some());
                    (title, description, available)
                })
                .collect();
            (names, text)
        }));
        let inventory = self
            .plugin_catalog
            .inventory
            .get_or_insert_with(Inventory::new);
        inventory.sync(content, window, cx);
        let state = inventory.list.clone();
        let owner = cx.entity();
        div()
            .id(("settings", Section::Plugins as usize))
            .debug_selector(|| "settings-viewport".into())
            .size_full()
            .min_h_0()
            .min_w_0()
            .relative()
            .overflow_hidden()
            .text_color(cx.theme().foreground)
            .child(
                gpui_kit::list(state.clone(), move |index, _, cx| {
                    owner.update(cx, |owner, cx| owner.inventory_row(index, cx))
                })
                .size_full(),
            )
            .child(Scrollbar::vertical(&state).mode(ScrollbarMode::Scrolling))
            .into_any_element()
    }

    fn inventory_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let row = &self.plugin_catalog.inventory.as_ref().unwrap().rows[index];
        if index == 0 {
            return div()
                .w_full()
                .track_focus(&row.focus)
                .child(
                    crate::ui::settings_page::SettingsPage::new("settings", tr("settings_plugins"))
                        .leading(crate::theme::banner("banner.settings", cx))
                        .description(tr("settings_plugins_description"))
                        .child(self.inventory_controls(cx))
                        .content(cx)
                        .when(
                            self.plugin_catalog.inventory.as_ref().unwrap().rows.len() > 1,
                            |content| content.pb_4(),
                        ),
                )
                .into_any_element();
        }
        let final_row = index + 1 == self.plugin_catalog.inventory.as_ref().unwrap().rows.len();
        let busy = !self.provider_link.as_ref().unwrap().connected || self.plugin_catalog.busy(cx);
        div()
            .w_full()
            .track_focus(&row.focus)
            .child(
                div()
                    .w_full()
                    .max_w(px(crate::preview::CONTENT_WIDTH))
                    .mx_auto()
                    .px_6()
                    .pb_3()
                    .when(final_row, |row| row.pb_6())
                    .child(
                        resource_card::grid()
                            .debug_selector(move || {
                                if index == 1 {
                                    "plugin-cards".into()
                                } else {
                                    format!("plugin-card-row-{index}")
                                }
                            })
                            .children(
                                row.names
                                    .iter()
                                    .filter_map(|name| {
                                        self.plugin_catalog
                                            .packages
                                            .iter()
                                            .find(|plugin| plugin.name == *name)
                                    })
                                    .map(|plugin| self.plugin_card(plugin, busy, cx)),
                            ),
                    ),
            )
            .into_any_element()
    }
}
