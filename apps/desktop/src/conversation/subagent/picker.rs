use super::*;
use gpui_kit::component::{
    button::Button,
    list::ListItem,
    popover::{Popover, PopoverState},
    tree::{TreeItem, TreeState, tree},
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use std::collections::BTreeSet;
mod source;
use source::Source;
pub(in crate::conversation) use source::{Choice, Row};

pub(in crate::conversation) fn live(
    owner: WeakEntity<super::super::live::View>,
    turn: sailry_protocol::TurnId,
    selected: Option<sailry_protocol::SessionId>,
    id: &'static str,
    button: Button,
) -> Popover {
    popover(
        Source::Live { owner, turn },
        selected.map(Choice::Live),
        id,
        button,
    )
}

impl Shell {
    pub(super) fn subagent_picker(
        &self,
        location: Location,
        id: &'static str,
        button: Button,
        cx: &mut Context<Self>,
    ) -> Popover {
        let selected = location.child.map(Choice::Preview);
        let location = Location {
            child: None,
            ..location
        };
        let owner = cx.entity().downgrade();
        popover(Source::Preview { location, owner }, selected, id, button)
    }
}

fn popover(source: Source, selected: Option<Choice>, id: &'static str, button: Button) -> Popover {
    Popover::new(id)
        .anchor(if selected.is_some() {
            Anchor::TopLeft
        } else {
            Anchor::BottomLeft
        })
        .map(|popover| {
            if selected.is_some() {
                popover.top_2()
            } else {
                popover.bottom_2()
            }
        })
        .p_2()
        .trigger(button)
        .content(move |_, window, cx| {
            let dismiss = cx.entity().downgrade();
            window
                .use_keyed_state(source.cache_key(), cx, |window, cx| {
                    Picker::new(source.clone(), selected, dismiss, window, cx)
                })
                .into_any_element()
        })
}

struct Picker {
    source: Source,
    rows: Vec<Row>,
    tree: Entity<TreeState>,
    dismiss: WeakEntity<PopoverState>,
}

impl Picker {
    fn new(
        source: Source,
        selected: Option<Choice>,
        dismiss: WeakEntity<PopoverState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let rows = source.rows(cx);
        let items = tree_items(&rows);
        let tree = cx.new(|cx| TreeState::new(cx).items(items));
        tree.update(cx, |state, cx| {
            if let Some(selected) = selected {
                state.set_selected_item(Some(&TreeItem::new(selected.tree_id(), "")), cx);
            } else {
                state.set_selected_index(Some(0), cx);
            }
            state.focus(window, cx);
        });
        cx.observe(&tree, |_, _, cx| cx.notify()).detach();
        source.observe(cx);
        Self {
            source,
            rows,
            tree,
            dismiss,
        }
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let rows = self.source.rows(cx);
        // Keyed state invalidates its rendering owner; unchanged snapshots must not echo it.
        if rows
            .iter()
            .map(|row| (row.key, row.parent, &row.name, row.status, row.order))
            .eq(self
                .rows
                .iter()
                .map(|row| (row.key, row.parent, &row.name, row.status, row.order)))
        {
            return;
        }
        if rows
            .iter()
            .map(|row| (row.key, row.parent, &row.name, row.order))
            .ne(self
                .rows
                .iter()
                .map(|row| (row.key, row.parent, &row.name, row.order)))
        {
            self.tree.update(cx, |state, cx| {
                let selected = state.selected_item().cloned();
                state.set_items(tree_items(&rows), cx);
                state.set_selected_item(selected.as_ref(), cx);
            });
        }
        self.rows = rows;
        cx.notify();
    }

    fn open(&mut self, key: Choice, window: &mut Window, cx: &mut Context<Self>) {
        self.source.open(key, cx);
        _ = self
            .dismiss
            .update(cx, |state, cx| state.dismiss(window, cx));
    }
}

impl Render for Picker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let owner = cx.entity().downgrade();
        let rows = self.rows.clone();
        let visible = (0..6)
            .take_while(|&index| self.tree.read(cx).entry(index).is_some())
            .count()
            .max(1);
        v_flex()
            .debug_selector(|| "subagent-picker".into())
            .w(px(360.).min(window.viewport_size().width - px(48.)))
            .id("subagent-choices")
            .capture_action(cx.listener(
                |picker, _: &gpui_kit::base::actions::Confirm, window, cx| {
                    let selected = picker.tree.read(cx).selected_entry().and_then(|entry| {
                        picker
                            .rows
                            .iter()
                            .find(|row| row.key.tree_id() == entry.item().id)
                            .map(|row| row.key)
                    });
                    if let Some(selected) = selected {
                        cx.stop_propagation();
                        picker.open(selected, window, cx);
                    }
                },
            ))
            .child(
                tree(&self.tree, move |index, entry, selected, _, cx| {
                    let row = rows
                        .iter()
                        .find(|row| row.key.tree_id() == entry.item().id)
                        .expect("tree item belongs to inventory")
                        .clone();
                    let key = row.key;
                    let owner = owner.clone();
                    let folder = entry.is_folder();
                    ListItem::new(index)
                        .selected(selected)
                        .rounded(cx.theme().radius)
                        .h_10()
                        .px_2()
                        .pl(px(16.) * entry.depth() + px(8.))
                        .debug_selector(move || format!("subagent-row-{}", key.debug_id()))
                        .tooltip(move |window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(crate::tr(row.status))
                                .build(window, cx)
                        })
                        .child(
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .gap_2()
                                .child(if folder {
                                    Icon::new(if entry.is_expanded() {
                                        IconName::ChevronDown
                                    } else {
                                        IconName::ChevronRight
                                    })
                                    .size_4()
                                    .into_any_element()
                                } else {
                                    div().w_4().into_any_element()
                                })
                                .child(crate::ui::identicon::agent(&key.debug_id(), cx))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_sm()
                                        .child(row.name.clone()),
                                )
                                .child(super::status_icon(&row)),
                        )
                        .on_click(move |event, window, cx| {
                            if !folder || event.click_count() == 2 {
                                _ = owner.update(cx, |picker, cx| picker.open(key, window, cx));
                            }
                        })
                })
                .h(px((visible * 40) as f32)),
            )
    }
}

fn tree_items(rows: &[Row]) -> Vec<TreeItem> {
    fn visit(row: &Row, rows: &[&Row], visited: &mut BTreeSet<Choice>) -> Option<TreeItem> {
        if !visited.insert(row.key) {
            return None;
        }
        let children = rows
            .iter()
            .filter(|child| child.parent == Some(row.key))
            .filter_map(|child| visit(child, rows, visited))
            .collect::<Vec<_>>();
        Some(
            TreeItem::new(row.key.tree_id(), row.name.clone())
                .expanded(true)
                .children(children),
        )
    }
    let mut ordered: Vec<_> = rows.iter().collect();
    ordered.sort_by_key(|row| (std::cmp::Reverse(row.order), row.key));
    let mut visited = BTreeSet::new();
    let mut items = Vec::new();
    for row in &ordered {
        if row
            .parent
            .is_none_or(|parent| parent == row.key || !rows.iter().any(|row| row.key == parent))
            && let Some(item) = visit(row, &ordered, &mut visited)
        {
            items.push(item);
        }
    }
    // Keep malformed cycles visible without recursing back into an already displayed item.
    for row in &ordered {
        if let Some(item) = visit(row, &ordered, &mut visited) {
            items.push(item);
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    fn row(id: usize, parent: Option<usize>, order: u64) -> Row {
        Row {
            key: Choice::Preview(Key { id, generation: 0 }),
            parent: parent.map(|id| Choice::Preview(Key { id, generation: 0 })),
            name: format!("Agent {id}").into(),
            status: "turn_waiting",
            icon: IconName::CircleUser,
            order,
        }
    }

    fn flatten(items: &[TreeItem]) -> Vec<SharedString> {
        items
            .iter()
            .flat_map(|item| std::iter::once(item.id.clone()).chain(flatten(&item.children)))
            .collect()
    }

    #[test]
    fn orders_newest_siblings() {
        let items = tree_items(&[
            row(0, None, 1),
            row(1, Some(0), 3),
            row(2, Some(0), 4),
            row(3, None, 2),
        ]);
        assert_eq!(flatten(&items), ["3:0", "0:0", "2:0", "1:0"]);
        assert_eq!(items[1].children.len(), 2);
        assert!(items[1].is_expanded());
    }

    #[test]
    fn deduplicates_invalid_ancestry() {
        let items = tree_items(&[
            row(0, Some(7), 5),
            row(1, Some(1), 4),
            row(2, Some(3), 3),
            row(3, Some(2), 2),
        ]);
        assert_eq!(flatten(&items), ["0:0", "1:0", "2:0", "3:0"]);
    }
}
