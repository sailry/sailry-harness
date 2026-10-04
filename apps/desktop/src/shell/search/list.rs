//! Catalog identities remain attached to each visible native picker row.
use super::*;
use catalog::{Choice, Group};
use gpui_kit::component::list::{ListDelegate, ListItem};

pub(super) struct Rows {
    owner: WeakEntity<Palette>,
    groups: Vec<Group>,
    visible: Vec<Group>,
    query: String,
    pub(super) selected: Option<IndexPath>,
}

impl Rows {
    pub(super) fn new(owner: WeakEntity<Palette>, groups: Vec<Group>) -> Self {
        let mut rows = Self {
            owner,
            groups,
            visible: Vec::new(),
            query: String::new(),
            selected: None,
        };
        rows.filter();
        rows
    }
    fn choice(&self, index: IndexPath) -> Option<&Choice> {
        self.visible.get(index.section)?.choices.get(index.row)
    }
    fn filter(&mut self) {
        let query = self.query.to_lowercase();
        self.visible = self
            .groups
            .iter()
            .filter_map(|group| {
                let choices: Vec<_> = group
                    .choices
                    .iter()
                    .filter(|choice| {
                        choice.label.to_lowercase().contains(&query)
                            || choice
                                .keywords
                                .iter()
                                .any(|key| key.to_lowercase().contains(&query))
                    })
                    .cloned()
                    .collect();
                (!choices.is_empty()).then(|| Group {
                    label: group.label.clone(),
                    choices,
                })
            })
            .collect();
        self.selected = (!self.visible.is_empty()).then_some(IndexPath::default());
    }
    pub(super) fn replace(&mut self, groups: Vec<Group>) {
        let target = self
            .selected
            .and_then(|index| self.choice(index))
            .map(|choice| choice.target.clone());
        self.groups = groups;
        self.filter();
        if let Some(target) = target {
            self.selected = self
                .visible
                .iter()
                .enumerate()
                .find_map(|(section, group)| {
                    group
                        .choices
                        .iter()
                        .position(|choice| choice.target.same(&target))
                        .map(|row| IndexPath::new(row).section(section))
                })
                .or(self.selected);
        }
    }
    pub(super) fn count(&self) -> usize {
        self.visible.iter().map(|group| group.choices.len()).sum()
    }
    pub(super) fn headings(&self) -> usize {
        self.visible
            .iter()
            .filter(|group| group.label.is_some())
            .count()
    }
}

impl ListDelegate for Rows {
    type Item = ListItem;
    fn sections_count(&self, _: &App) -> usize {
        self.visible.len().max(1)
    }
    fn items_count(&self, section: usize, _: &App) -> usize {
        self.visible
            .get(section)
            .map_or(0, |group| group.choices.len())
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
        let query = self.query.clone();
        let selected = self.selected;
        let owner = cx.entity().downgrade();
        // List resets selection after the delegate returns; restore the first actual match
        // outside that lease, including searches that recover from an empty result.
        window.defer(cx, move |window, cx| {
            let _ = owner.update(cx, |list, cx| {
                if list.delegate().query == query {
                    list.set_selected_index(selected, window, cx);
                    cx.notify();
                }
            });
        });
        cx.notify();
        Task::ready(())
    }
    fn render_section_header(
        &mut self,
        section: usize,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<impl IntoElement> {
        self.visible
            .get(section)?
            .label
            .clone()
            .map(|label| crate::command_picker::heading(label, cx))
    }
    fn render_item(
        &mut self,
        index: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let choice = self.choice(index)?;
        let selector = format!("global-command-item-{}-{}", index.section, index.row);
        Some(
            crate::command_picker::row(
                SharedString::from(selector.clone()),
                choice.selected,
                false,
                cx,
            )
            .debug_selector(move || selector.clone())
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
                    .child(
                        div()
                            .w_4()
                            .flex_shrink_0()
                            .child(choice.icon.clone().size_4()),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_sm()
                            .child(choice.label.clone()),
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
            .p_3()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(tr("connection_search_empty"))
    }
    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let Some(target) = self
            .selected
            .and_then(|index| self.choice(index))
            .map(|choice| choice.target.clone())
        else {
            return;
        };
        let owner = self.owner.clone();
        window.defer(cx, move |window, cx| {
            let _ = owner.update(cx, |palette, cx| palette.confirm(target, window, cx));
        });
    }
    fn cancel(&mut self, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let has_query = !self.query.is_empty()
            || window
                .focused_input(cx)
                .is_some_and(|input| !input.value(cx).is_empty());
        if !has_query {
            return;
        }
        cx.stop_propagation();
        let owner = cx.entity().downgrade();
        window.defer(cx, move |window, cx| {
            let _ = owner.update(cx, |list, cx| list.set_query("", window, cx));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Section;
    use core::prelude::v1::test;

    fn choice(label: &str, target: Section) -> Choice {
        Choice {
            label: label.to_owned().into(),
            icon: IconName::Settings.into(),
            keywords: vec!["needle".into()],
            selected: false,
            target: Target::Setting(target),
        }
    }

    #[gpui::test]
    fn search_and_refresh_keep_target_identity(cx: &mut TestAppContext) {
        let (shell, mut visual) = crate::shell::tests::setup(cx);
        let palette = visual.update(|window, cx| {
            let scope = Scope::Preview(0);
            let groups = catalog::entries(shell.read(cx), scope, Folder::Root, cx);
            cx.new(|cx| Palette::new(shell, scope, groups, window, cx))
        });
        let groups = vec![
            Group {
                label: Some("First".into()),
                choices: vec![choice("Base", Section::General)],
            },
            Group {
                label: Some("Second".into()),
                choices: vec![choice("Connections", Section::Providers)],
            },
        ];
        let mut rows = Rows::new(palette.downgrade(), groups);
        rows.query = "NEEDLE".into();
        rows.filter();
        assert_eq!(rows.count(), 2);
        assert_eq!(rows.headings(), 2);
        rows.selected = Some(IndexPath::new(0).section(1));
        rows.replace(vec![Group {
            label: Some("Updated".into()),
            choices: vec![choice("Renamed", Section::Providers)],
        }]);
        assert_eq!(rows.count(), 1);
        assert_eq!(rows.selected, Some(IndexPath::default()));
        assert!(matches!(
            rows.choice(rows.selected.unwrap()).unwrap().target,
            Target::Setting(Section::Providers)
        ));
        rows.query = "absent".into();
        rows.filter();
        assert_eq!(rows.count(), 0);
        assert!(rows.selected.is_none());
        rows.query.clear();
        rows.filter();
        assert_eq!(rows.count(), 1);
        assert_eq!(rows.selected, Some(IndexPath::default()));
    }
}
