//! One composer suggestion list serves live references and UI commands.
//! References reuse existing worktree commands and the immutable session role roster.
//! Like Code 67ae9fa0, ordinary references are context text, not another file/history store.
use super::*;
use crate::conversation::references::Trigger;
use catalog::{Item, Page, Reference, Target};
use gpui_kit::component::list::ListState;
use view::Items;

mod catalog;
pub(super) mod commands;
mod databases;
mod details;
pub(super) mod inline;
mod skills;
#[cfg(test)]
mod tests;
mod view;

pub(super) struct State {
    // Retain targets until the draft is sent so Undo can restore removed labels.
    pub selected: Vec<Reference>,
    pub(super) command_keys: BTreeMap<String, crate::plugins::contributions::Key>,
    bounds: Bounds<Pixels>,
    open: bool,
    trigger: Option<Trigger>,
    dismissed: Option<Trigger>,
    page: Page,
    rows: Vec<Item>,
    list: Entity<ListState<Items>>,
    generation: u64,
    next: Option<sailry_protocol::DirectoryCursor>,
    loading: bool,
    error: bool,
    stop: CancellationToken,
    task: Option<Task<()>>,
}

impl Drop for State {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl State {
    pub(super) fn failure(&self) -> Option<&'static str> {
        self.error.then_some(match self.page {
            Page::Commands | Page::Root => "plugins_read_failed",
            Page::Databases | Page::Database(..) => "db_failed",
            Page::Ssh => "ssh_failed",
            _ => "files_read_failed",
        })
    }

    pub fn new(owner: WeakEntity<View>, window: &mut Window, cx: &mut App) -> Self {
        Self {
            selected: vec![],
            command_keys: BTreeMap::new(),
            bounds: Bounds::default(),
            open: false,
            trigger: None,
            dismissed: None,
            page: Page::Root,
            rows: vec![],
            list: cx.new(|cx| ListState::new(Items::new(owner), window, cx).searchable(false)),
            generation: 0,
            next: None,
            loading: false,
            error: false,
            stop: CancellationToken::new(),
            task: None,
        }
    }

    pub(super) fn dismiss(&mut self) {
        self.open = false;
        self.dismissed = self.trigger.clone();
        self.stop.cancel();
        self.generation += 1;
    }
}

impl View {
    pub(super) fn inherit_command_keys(&mut self, previous: &State) {
        self.references.command_keys = previous.command_keys.clone();
    }

    pub(super) fn refresh_references(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let trigger = self.input.update(cx, |input, cx| {
            if input.marked_text_range(window, cx).is_some() {
                return None;
            }
            Trigger::parse(input.value(), input.selected_range())
                .or_else(|| Trigger::command(input.value(), input.selected_range()))
                .filter(|trigger| {
                    !input.tokens().iter().any(|span| {
                        let range = span.range();
                        range.start < trigger.range.end && trigger.range.start < range.end
                    })
                })
        });
        if self.references.trigger == trigger {
            return;
        }
        let fresh = !self.references.open
            || self
                .references
                .trigger
                .as_ref()
                .map(|token| (token.range.start, token.text.as_bytes()[token.range.start]))
                != trigger
                    .as_ref()
                    .map(|token| (token.range.start, token.text.as_bytes()[token.range.start]));
        if trigger != self.references.dismissed {
            self.references.dismissed = None;
        }
        self.references.trigger = trigger;
        self.references.open = self.references.trigger.is_some()
            && self.references.trigger != self.references.dismissed
            && !self.pending
            && self.retry.is_none();
        if self.references.open {
            if fresh {
                self.reference_page(self.reference_root(), window, cx);
            } else {
                self.reference_rows(window, cx);
            }
        } else {
            self.references.stop.cancel();
        }
        cx.notify();
    }

    fn reference_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        self.references.stop.cancel();
        self.references.generation += 1;
        self.references.rows = self.reference_catalog(&page, cx);
        self.references.page = page.clone();
        self.references.next = None;
        self.references.error = false;
        self.references.loading = false;
        if matches!(page, Page::Files(_)) {
            self.load_references(false, window, cx);
        } else if matches!(page, Page::Database(..)) {
            self.load_database_references(window, cx);
        } else if matches!(page, Page::Commands | Page::Root) && self.composer_options.skills {
            self.load_skills(window, cx);
        }
        self.reference_rows(window, cx);
    }

    fn load_references(&mut self, more: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(worktree) = self.binding.worktree else {
            return;
        };
        let Page::Files(path) = self.references.page.clone() else {
            return;
        };
        self.references.stop.cancel();
        let stop = self.stop.child_token();
        self.references.stop = stop.clone();
        self.references.generation += 1;
        let generation = self.references.generation;
        self.references.loading = true;
        self.references.error = false;
        let client = self.binding.client.clone();
        let command = Command::ListDirectory {
            worktree,
            path,
            after: if more {
                self.references.next.clone()
            } else {
                None
            },
        };
        let task = self.binding.runtime.spawn(async move {
            tokio::select! {
                _ = stop.cancelled() => None,
                result = client.execute(client.prepare(command)) => Some(result),
            }
        });
        self.references.task = Some(cx.spawn_in(window, async move |view, cx| {
            let Ok(Some(result)) = task.await else {
                return;
            };
            _ = view.update_in(cx, |view, window, cx| {
                if view.references.generation != generation || !view.references.open {
                    return;
                }
                view.references.loading = false;
                if let Ok(Output::Directory(directory)) = result {
                    let mut rows = catalog::files(&directory);
                    if more {
                        if !directory.path.is_empty() {
                            rows.remove(0);
                        }
                        view.references.rows.extend(rows);
                    } else {
                        view.references.rows = rows;
                    }
                    view.references.next = directory.next;
                } else {
                    view.references.error = true;
                }
                view.reference_rows(window, cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn reference_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self
            .references
            .trigger
            .as_ref()
            .map_or("", |trigger| trigger.query.as_str())
            .to_lowercase();
        let rows = self
            .references
            .rows
            .iter()
            .filter(|row| row.matches(&query))
            .cloned()
            .collect::<Vec<_>>();
        let generation = self.references.generation;
        self.references.list.update(cx, |list, cx| {
            let selected = (!rows.is_empty()).then_some(IndexPath::default());
            list.delegate_mut().rows = rows;
            list.delegate_mut().generation = generation;
            list.set_selected_index(selected, window, cx);
            list.scroll_to_selected_item(window, cx);
            cx.notify();
        });
    }

    pub(super) fn refresh_commands(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.references.open && self.references.page == Page::Commands {
            let skills: Vec<_> = self
                .references
                .rows
                .iter()
                .filter(|row| matches!(row, Item::Command(commands::Choice::Skill(..))))
                .cloned()
                .collect();
            self.references.rows = self.command_catalog(&Page::Commands, cx);
            self.references.rows.extend(skills);
            self.reference_rows(window, cx);
        }
        self.capture_command(cx);
    }

    pub(super) fn capture_command(&mut self, cx: &App) {
        let text = self.input.read(cx).value();
        let Some((name, _, _)) = commands::leading(&text) else {
            return;
        };
        if self.references.command_keys.contains_key(name) {
            return;
        }
        if let Some(entry) = self
            .contributions
            .read(cx)
            .commands(cx)
            .into_iter()
            .find(|entry| {
                entry.state.enabled
                    && entry.declaration.command.as_ref().is_some_and(|command| {
                        command.name == name
                            && command.kind == sailry_protocol::plugin::ui::CommandKind::Message
                    })
            })
        {
            self.references
                .command_keys
                .insert(name.to_owned(), entry.key);
        }
    }

    pub(super) fn command_valid(&self, cx: &App) -> bool {
        let text = self.input.read(cx).value();
        let Some((name, _, _)) = commands::leading(&text) else {
            return true;
        };
        self.references.command_keys.get(name).is_none_or(|key| {
            self.contributions
                .read(cx)
                .commands(cx)
                .iter()
                .any(|entry| entry.key == *key && entry.state.enabled)
        })
    }

    fn choose_reference(
        &mut self,
        generation: u64,
        item: Item,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.references.open
            || self.input.update(cx, |input, cx| {
                input.marked_text_range(window, cx).is_some()
            })
            || self.references.generation != generation
            || self.pending
            || self.retry.is_some()
        {
            return;
        }
        let Some(trigger) = self.references.trigger.clone() else {
            return;
        };
        if self.input.read(cx).value() != trigger.text
            || self.input.read(cx).selected_range() != (trigger.range.end..trigger.range.end)
        {
            self.references.dismiss();
            cx.notify();
            return;
        }
        let item = if let Item::Plugin(info) = item {
            Item::Reference(Reference {
                label: crate::plugins::metadata::title(&info),
                target: Target::Plugin(info.summary.name),
            })
        } else {
            item
        };
        let page = match item {
            Item::Plugin(_) => unreachable!(),
            Item::Attachment => {
                if self.attachments_blocked() {
                    return;
                }
                self.references.dismiss();
                self.input.update(cx, |input, cx| {
                    input.set_selected_range(trigger.range, cx);
                    input.replace("", window, cx);
                    input.focus(window, cx);
                });
                self.choose_attachments(window, cx);
                cx.notify();
                return;
            }
            Item::Command(command) => {
                self.choose_command(command, trigger, window, cx);
                return;
            }
            Item::Page(page) => Some(page),
            Item::Reference(reference) | Item::Current(reference) => {
                if matches!(reference.target, Target::Agent(_))
                    && self.active_references(cx).iter().any(|item| {
                        matches!(item.target, Target::Agent(_)) && item.target != reference.target
                    })
                {
                    self.error = Some("reference_one_agent");
                    cx.notify();
                    return;
                }
                let token = self.remember_reference(reference);
                self.references.dismiss();
                let result = self.input.update(cx, |input, cx| {
                    let result = input.replace_range_with_token(trigger.range, token, window, cx);
                    if result.is_ok() {
                        input.replace(" ", window, cx);
                    }
                    input.focus(window, cx);
                    result
                });
                self.error = result.err().map(|_| "reference_stale");
                cx.notify();
                return;
            }
        };
        let marker = if self.reference_root() == Page::Commands {
            "/"
        } else {
            "@"
        };
        self.input.update(cx, |input, cx| {
            input.set_selected_range(trigger.range, cx);
            input.replace(if page.is_some() { marker } else { "" }, window, cx);
            input.focus(window, cx);
        });
        self.refresh_references(window, cx);
        if let Some(page) = page {
            self.reference_page(page, window, cx);
        }
        self.error = None;
        cx.notify();
    }

    fn reference_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let page = match &self.references.page {
            Page::Files(path) if !path.is_empty() => Page::Files(
                path.rsplit_once('/')
                    .map_or("", |(parent, _)| parent)
                    .into(),
            ),
            Page::Database(id, name, Some(_)) => Page::Database(*id, name.clone(), None),
            Page::Database(_, _, None)
                if matches!(self.composer_options.mentions, Mentions::Workspace) =>
            {
                Page::Databases
            }
            _ => self.reference_root(),
        };
        self.choose_reference(self.references.generation, Item::Page(page), window, cx);
    }

    pub(super) fn reference_action(
        &mut self,
        action: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.references.open
            || self.input.update(cx, |input, cx| {
                input.marked_text_range(window, cx).is_some()
            })
        {
            cx.propagate();
            return;
        }
        let list = self.references.list.clone();
        match action {
            "up" | "down" => list.update(cx, |list, cx| {
                let count = list.delegate().rows.len();
                if count > 0 {
                    let current = list
                        .selected_index()
                        .and_then(|index| list.delegate().flat_index(index))
                        .unwrap_or(0);
                    let next = (current + if action == "up" { count - 1 } else { 1 }) % count;
                    let index = list.delegate().index(next);
                    list.set_selected_index(Some(index), window, cx);
                    list.scroll_to_selected_item(window, cx);
                    cx.notify();
                }
            }),
            "enter" | "tab" => {
                let item = list
                    .read(cx)
                    .selected_index()
                    .and_then(|index| list.read(cx).delegate().item(index))
                    .cloned();
                if let Some(item) = item {
                    self.choose_reference(self.references.generation, item, window, cx);
                } else if !self.references.loading {
                    cx.propagate();
                    return;
                }
            }
            "escape" => self.references.dismiss(),
            "backspace"
                if self.references.page != self.reference_root()
                    && self
                        .references
                        .trigger
                        .as_ref()
                        .is_some_and(|token| token.query.is_empty()) =>
            {
                self.reference_back(window, cx)
            }
            _ => {
                cx.propagate();
                return;
            }
        }
        cx.stop_propagation();
        window.prevent_default();
        cx.notify();
    }
}

impl View {
    pub(crate) fn insert_file_references(
        &mut self,
        paths: Vec<(String, bool)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.input.update(cx, |input, cx| {
            input.marked_text_range(window, cx).is_some()
        }) {
            return;
        }
        for (path, directory) in paths {
            let reference = Reference {
                label: path.clone(),
                target: if directory {
                    Target::Directory(path)
                } else {
                    Target::File(path)
                },
            };
            let token = self.remember_reference(reference);
            self.input.update(cx, |input, cx| {
                let value = input.value();
                let start = input.selected_range().start;
                let separator = if value[..start]
                    .chars()
                    .next_back()
                    .is_some_and(|ch| !ch.is_whitespace())
                {
                    " "
                } else {
                    ""
                };
                if !separator.is_empty() {
                    input.replace(separator, window, cx);
                }
                if input.replace_with_token(token, window, cx).is_ok() {
                    input.replace(" ", window, cx);
                }
            });
        }
        self.references.dismiss();
        self.input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }
}

impl View {
    pub(crate) fn can_request_file_edit(&self, worktree: Option<WorktreeId>) -> bool {
        !self.readonly()
            && !self.pending
            && self.retry.is_none()
            && worktree.is_some()
            && worktree == self.binding.worktree
    }

    pub(crate) fn request_file_edit(
        &mut self,
        worktree: Option<WorktreeId>,
        path: String,
        page: u32,
        selection: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_request_file_edit(worktree) {
            return;
        }
        if self.input.update(cx, |input, cx| {
            input.marked_text_range(window, cx).is_some()
        }) {
            return;
        }
        let token = self.remember_reference(Reference {
            label: path.clone(),
            target: Target::File(path),
        });
        let mut context = String::new();
        if !selection.trim().is_empty() {
            let selection: String = selection.chars().take(16000).collect();
            context.push_str(&format!(
                "\n{}\n{}",
                rust_i18n::t!("artifact_selection_context", page = page),
                selection
            ));
        }
        context.push_str(&format!("\n{}", tr("artifact_edit_prompt")));
        self.input.update(cx, |input, cx| {
            let end = input.value().len();
            input.set_selected_range(end..end, cx);
            if end > 0 {
                input.replace("\n\n", window, cx);
            }
            if input.replace_with_token(token, window, cx).is_ok() {
                input.replace(context, window, cx);
            }
            input.focus(window, cx);
        });
        self.references.dismiss();
        cx.notify();
    }
}
