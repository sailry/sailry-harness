//! Header filter declarations reuse Kit Select, TabBar and Popover, including narrow layouts.
use super::*;
use gpui_kit::component::{
    popover::Popover,
    select::{SearchableVec, Select, SelectEvent, SelectItem, SelectState},
    tab::{Tab, TabBar},
};
use std::collections::BTreeMap;

#[derive(Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct Filter {
    id: String,
    label: String,
    items: Vec<Choice>,
    selected: String,
    #[serde(default)]
    disabled: bool,
    #[serde(default)]
    primary: bool,
    #[serde(default)]
    searchable: bool,
    /// Shows a few short choices as segmented tabs instead of a select.
    #[serde(default)]
    segmented: bool,
}
#[derive(Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Choice {
    id: String,
    label: String,
}
impl SelectItem for Choice {
    type Value = String;
    fn title(&self) -> SharedString {
        self.label.clone().into()
    }
    fn value(&self) -> &String {
        &self.id
    }
}
struct Field {
    props: Filter,
    state: Entity<SelectState<SearchableVec<Choice>>>,
    _events: Subscription,
}
#[derive(Default)]
pub(super) struct Controls {
    fields: BTreeMap<String, Field>,
    compact: bool,
}

impl Filter {
    pub(super) fn valid(&self) -> bool {
        !self.id.is_empty()
            && self.id.len() <= 128
            && self.label.len() <= 1024
            && self.items.len() <= 1024
            && self
                .items
                .iter()
                .all(|item| item.id.len() <= 512 && item.label.len() <= 4096)
    }
}
impl Header {
    #[cfg(test)]
    pub(in crate::plugins) fn filter_id(&self, id: &str) -> Option<ElementId> {
        self.filters
            .fields
            .get(id)
            .map(|field| ("select", field.state.entity_id()).into())
    }

    #[cfg(test)]
    pub(in crate::plugins) fn filter_interaction(
        &self,
        id: &str,
        window: &Window,
        cx: &App,
    ) -> Option<(Option<usize>, Option<String>, bool)> {
        self.filters.fields.get(id).map(|field| {
            let state = field.state.read(cx);
            (
                state.selected_index(cx).map(|index| index.row),
                state.selected_value().cloned(),
                state.focus_handle(cx).is_focused(window),
            )
        })
    }

    #[cfg(test)]
    pub(in crate::plugins) fn filter_ready(&self, id: &str, cx: &App) -> bool {
        self.filters.fields.get(id).is_some_and(|field| {
            !field.props.disabled
                && field.state.read(cx).selected_value() == Some(&field.props.selected)
        })
    }

    #[cfg(test)]
    pub(in crate::plugins) fn filter_selected(&self, id: &str) -> Option<&str> {
        self.filters
            .fields
            .get(id)
            .map(|field| field.props.selected.as_str())
    }

    #[cfg(test)]
    pub(crate) fn filter_items(&self, id: &str) -> Vec<String> {
        self.filters
            .fields
            .get(id)
            .map(|field| {
                field
                    .props
                    .items
                    .iter()
                    .map(|choice| choice.label.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn sync_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filters.fields.retain(|id, field| {
            self.content
                .filters
                .iter()
                .any(|filter| &filter.id == id && filter.searchable == field.props.searchable)
        });
        for props in &self.content.filters {
            let field = self
                .filters
                .fields
                .entry(props.id.clone())
                .or_insert_with(|| {
                    let state = cx.new(|cx| {
                        SelectState::new(SearchableVec::new(props.items.clone()), None, window, cx)
                            .searchable(props.searchable)
                    });
                    let id = props.id.clone();
                    let events = self.actions.clone();
                    let subscription = cx.subscribe(&state, move |_, _, event, _| {
                        if let SelectEvent::Confirm(Some(value)) = event {
                            let _ = events
                                .try_send(serde_json::json!({"id":id,"value":value}).to_string());
                        }
                    });
                    state.update(cx, |state, cx| {
                        state.set_selected_value(&props.selected, window, cx)
                    });
                    Field {
                        props: props.clone(),
                        state,
                        _events: subscription,
                    }
                });
            if field.props.items != props.items {
                field.state.update(cx, |state, cx| {
                    state.set_items(SearchableVec::new(props.items.clone()), window, cx)
                });
            }
            if field.props != *props {
                field.state.update(cx, |state, cx| {
                    state.set_selected_value(&props.selected, window, cx)
                });
                field.props = props.clone();
            }
        }
    }

    pub(super) fn controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let owner = cx.entity();
        let compact = self.filters.compact;
        h_flex()
            .w_full()
            .min_w_0()
            .gap_1()
            .when(self.trailing, |row| row.justify_end())
            .debug_selector(|| "plugin-header-filters".into())
            .on_prepaint(move |bounds, _, cx| {
                let owner = owner.clone();
                cx.defer(move |cx| {
                    owner.update(cx, |owner, cx| {
                        let compact = bounds.size.width < px(660.);
                        if owner.filters.compact != compact {
                            owner.filters.compact = compact;
                            cx.notify();
                        }
                    })
                });
            })
            .children(
                self.content
                    .filters
                    .iter()
                    .filter(|filter| filter.primary || !compact)
                    .filter_map(|filter| self.filters.fields.get(&filter.id))
                    .map(|field| field.render(false, &self.actions)),
            )
            .when(
                compact && self.content.filters.iter().any(|filter| !filter.primary),
                |row| {
                    let owner = cx.entity();
                    let label = self.content.filters_label.clone();
                    row.child(
                        Popover::new("plugin-header-overflow")
                            .w(rems(17.))
                            .trigger(
                                Button::new("plugin-header-filters-trigger")
                                    .debug_selector(|| "plugin-header-filters-trigger".into())
                                    .ghost()
                                    .small()
                                    .icon(IconName::Settings2)
                                    .tooltip(label.clone())
                                    .accessibility_label(label),
                            )
                            .content(move |_, _, cx| {
                                let owner = owner.read(cx);
                                v_flex().w_full().gap_1().children(
                                    owner
                                        .content
                                        .filters
                                        .iter()
                                        .filter(|filter| !filter.primary)
                                        .filter_map(|filter| owner.filters.fields.get(&filter.id))
                                        .map(|field| field.render(true, &owner.actions)),
                                )
                            }),
                    )
                },
            )
            .into_any_element()
    }
}
impl Field {
    fn render(&self, compact: bool, events: &tokio::sync::mpsc::Sender<String>) -> Div {
        if self.props.segmented {
            return self.segments(events.clone());
        }
        let id = self.props.id.clone();
        div()
            .w(rems(7.))
            .when(compact, |view| view.w_full())
            .min_w_0()
            .flex_shrink_0()
            .debug_selector(move || id.clone())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                Select::new(&self.state)
                    .small()
                    .appearance(false)
                    .menu_width(rems(15.))
                    .menu_max_h(rems(16.))
                    .accessibility_label(self.props.label.clone())
                    .disabled(self.props.disabled),
            )
    }

    fn segments(&self, events: tokio::sync::mpsc::Sender<String>) -> Div {
        let id = self.props.id.clone();
        let values: Vec<_> = self
            .props
            .items
            .iter()
            .map(|item| item.id.clone())
            .collect();
        let selected = values
            .iter()
            .position(|value| value == &self.props.selected)
            .unwrap_or(usize::MAX);
        let disabled = self.props.disabled;
        div()
            .flex_shrink_0()
            .debug_selector({
                let id = id.clone();
                move || id.clone()
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                TabBar::new(SharedString::from(format!("{id}-tabs")))
                    .segmented()
                    .small()
                    .menu(false)
                    .selected_index(selected)
                    .children(self.props.items.iter().map(|item| {
                        let selector = format!("{id}-item-{}", item.id);
                        Tab::new()
                            .label(item.label.clone())
                            .disabled(disabled)
                            .debug_selector(move || selector.clone())
                    }))
                    .on_click(move |index, _, _| {
                        if !disabled && let Some(value) = values.get(*index) {
                            let _ = events
                                .try_send(serde_json::json!({"id":id,"value":value}).to_string());
                        }
                    }),
            )
    }
}
