use super::*;
use crate::theme::DialogStyle as _;
use gpui_kit::component::{
    checkbox::Checkbox, dialog::DialogFooter, form::Field, scroll::ScrollableElement,
};
use sailry_protocol::plugin::Summary;
use std::collections::BTreeSet;

mod import;
mod source;
mod view;
use source::{Channel, Discovery, Selection};

struct Editor {
    binding: Binding,
    known: Vec<Summary>,
    target: Option<String>,
    fields: [Entity<InputState>; 3],
    links: [Entity<InputState>; 2],
    channel: Channel,
    imports: [Option<std::sync::Arc<import::Snapshot>>; 2],
    lease: Option<import::Lease>,
    discovery: Option<Discovery>,
    catalog_source: Option<Selection>,
    selected: BTreeSet<usize>,
    completed: BTreeSet<usize>,
    scroll: ScrollHandle,
    request: Option<(usize, Request)>,
    pending: bool,
    generation: u64,
    error: Option<&'static str>,
    closed: bool,
    dialog: crate::ui::overlays::Owned,
    task: Option<Task<()>>,
}

pub(super) fn open(binding: Binding, known: Vec<Summary>, window: &mut Window, cx: &mut App) {
    open_source(binding, known, Channel::Git, None, None, window, cx);
}

pub(super) fn reimport(
    binding: Binding,
    known: Vec<Summary>,
    summary: Summary,
    kind: sailry_protocol::plugin::UploadSource,
    window: &mut Window,
    cx: &mut App,
) {
    let channel = match kind {
        sailry_protocol::plugin::UploadSource::Directory => Channel::Directory,
        sailry_protocol::plugin::UploadSource::Archive => Channel::Archive,
    };
    open_source(
        binding,
        known,
        channel,
        Some(summary.name),
        None,
        window,
        cx,
    );
}

pub(in crate::settings) fn open_catalog(
    binding: Binding,
    known: Vec<Summary>,
    source: sailry_protocol::plugin::skillcatalog::InstallSource,
    window: &mut Window,
    cx: &mut App,
) {
    let (channel, source) = match source {
        sailry_protocol::plugin::skillcatalog::InstallSource::Git(source) => {
            (Channel::Git, Selection::Git { source })
        }
        sailry_protocol::plugin::skillcatalog::InstallSource::Registry(source) => {
            (Channel::Hub, Selection::Registry(source))
        }
    };
    let editor = open_source(binding, known, channel, None, Some(source), window, cx);
    editor.update(cx, |editor, cx| editor.discover(window, cx));
}

pub(in crate::settings) fn open_discovery(
    binding: Binding,
    known: Vec<Summary>,
    discovery: sailry_protocol::plugin::skills::Discovery,
    window: &mut Window,
    cx: &mut App,
) {
    let source = Selection::Git {
        source: sailry_protocol::plugin::skills::Source {
            repository: discovery.source.repository.clone(),
            git_ref: Some(discovery.source.commit.clone()),
            path: None,
        },
    };
    let editor = open_source(
        binding,
        known.clone(),
        Channel::Git,
        None,
        Some(source),
        window,
        cx,
    );
    editor.update(cx, |editor, cx| {
        editor.accept(Discovery::Git(discovery), known);
        cx.notify();
    });
}

pub(in crate::settings) fn open_registry_discovery(
    binding: Binding,
    known: Vec<Summary>,
    discovery: sailry_protocol::plugin::skills::RegistryDiscovery,
    window: &mut Window,
    cx: &mut App,
) {
    let source = Selection::Registry(sailry_protocol::plugin::skills::RegistrySource {
        namespace: Some(discovery.source.namespace.clone()),
        slug: discovery.source.slug.clone(),
        version: Some(discovery.source.version.clone()),
    });
    let editor = open_source(
        binding,
        known.clone(),
        Channel::Hub,
        None,
        Some(source),
        window,
        cx,
    );
    editor.update(cx, |editor, cx| {
        editor.accept(Discovery::Registry(discovery), known);
        cx.notify();
    });
}

fn open_source(
    binding: Binding,
    known: Vec<Summary>,
    channel: Channel,
    target: Option<String>,
    initial: Option<Selection>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Editor> {
    let values: [String; 3] = initial
        .as_ref()
        .and_then(|selection| {
            let Selection::Git { source } = selection else {
                return None;
            };
            Some([
                source.repository.clone(),
                source.git_ref.clone().unwrap_or_default(),
                source.path.clone().unwrap_or_default(),
            ])
        })
        .unwrap_or_default();
    let registry_label = match initial.as_ref() {
        Some(Selection::Registry(source)) => {
            let name = source.namespace.as_ref().map_or_else(
                || source.slug.clone(),
                |namespace| format!("{namespace}/{}", source.slug),
            );
            source
                .version
                .as_ref()
                .map_or(name.clone(), |version| format!("{name}@{version}"))
        }
        _ => String::new(),
    };
    let editor = cx.new(|cx| {
        let placeholders = [
            "skills_repository_hint",
            "skills_revision_hint",
            "skills_path_hint",
        ];
        let fields = std::array::from_fn(|index| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(tr(placeholders[index]))
                    .default_value(values[index].clone())
            })
        });
        let links = std::array::from_fn(|index| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(tr(if index == 0 {
                        "skill_source_skills_hint"
                    } else {
                        "skill_source_hub_hint"
                    }))
                    .default_value(if index == 1 {
                        registry_label.clone()
                    } else {
                        String::new()
                    })
            })
        });
        for input in fields.iter().chain(links.iter()) {
            cx.subscribe(input, |editor: &mut Editor, _, event, cx| {
                if matches!(event, InputEvent::Change)
                    && editor.request.is_none()
                    && !(editor.pending && editor.discovery.is_some())
                {
                    editor.invalidate();
                    cx.notify();
                }
            })
            .detach();
        }
        crate::feedback::observe(window, cx, |editor: &Editor, _| {
            editor.error.into_iter().collect()
        });
        Editor {
            binding,
            known,
            target,
            fields,
            links,
            channel,
            imports: [None, None],
            lease: None,
            discovery: None,
            catalog_source: initial,
            selected: BTreeSet::new(),
            completed: BTreeSet::new(),
            scroll: ScrollHandle::default(),
            request: None,
            pending: false,
            generation: 0,
            error: None,
            closed: false,
            dialog: Default::default(),
            task: None,
        }
    });
    view::open(editor.clone(), window, cx);
    editor
}

impl Editor {
    fn select_channel(&mut self, index: usize, cx: &mut Context<Self>) {
        let channel = Channel::from_index(index);
        if self.closed || self.pending || self.request.is_some() || self.channel == channel {
            return;
        }
        self.channel = channel;
        self.invalidate();
        cx.notify();
    }

    fn invalidate(&mut self) {
        self.scroll.set_offset(point(px(0.), px(0.)));
        self.generation = self.generation.wrapping_add(1);
        self.discovery = None;
        self.catalog_source = None;
        self.selected.clear();
        self.completed.clear();
        self.error = None;
        self.pending = false;
        self.task = None;
    }

    fn selection(&self, cx: &App) -> Option<Selection> {
        if let Some(source) = &self.catalog_source {
            return Some(source.clone());
        }
        match self.channel {
            Channel::Git => Selection::git(
                self.fields
                    .each_ref()
                    .map(|field| field.read(cx).value().trim().to_string()),
            ),
            Channel::Skills => Selection::skills(&self.links[0].read(cx).value()),
            Channel::Hub => Selection::hub(&self.links[1].read(cx).value()),
            Channel::Directory | Channel::Archive => None,
        }
    }

    fn ready(&self, cx: &App) -> bool {
        self.channel.local().map_or_else(
            || self.selection(cx).is_some(),
            |index| self.imports[index].is_some(),
        )
    }

    fn selected_installable(&self) -> bool {
        self.discovery.is_some()
            && self
                .selected
                .iter()
                .any(|index| !self.completed.contains(index))
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.request.is_some() || self.selected_installable() {
            self.install(window, cx);
        } else {
            self.discover(window, cx);
        }
    }

    fn discover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed || self.pending || self.request.is_some() {
            return;
        }
        if self.channel.local().is_some() {
            self.discover_import(window, cx);
            return;
        }
        let selection = self.selection(cx);
        let Some(selection) = selection else {
            self.error = Some(if self.channel == Channel::Git {
                "skills_source_invalid"
            } else {
                "skill_source_link_invalid"
            });
            cx.notify();
            return;
        };
        let command = selection.command();
        let generation = self.generation;
        self.pending = true;
        self.error = None;
        self.discovery = None;
        self.selected.clear();
        self.completed.clear();
        let binding = self.binding.clone();
        let job = binding.runtime.spawn(async move {
            let discovery = binding
                .client
                .execute(binding.client.prepare(command))
                .await?;
            let known = binding
                .client
                .execute(binding.client.prepare(Command::ListPlugins))
                .await?;
            Ok::<_, Fault>((discovery, known))
        });
        self.task = Some(cx.spawn_in(window, async move |editor, cx| {
            let result = job.await;
            _ = editor.update_in(cx, |editor, _, cx| {
                if editor.closed || editor.generation != generation {
                    return;
                }
                editor.pending = false;
                match result {
                    Ok(Ok((discovery, Output::Plugins(known)))) => {
                        let discovery = match selection.select(discovery) {
                            Ok(discovery) => discovery,
                            Err(_) => {
                                editor.error = Some("skill_source_unresolved");
                                cx.notify();
                                return;
                            }
                        };
                        editor.accept(discovery, known);
                    }
                    Ok(Err(error)) => editor.error = Some(error_key(&error)),
                    _ => editor.error = Some("skills_failed"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn accept(&mut self, mut discovery: Discovery, known: Vec<Summary>) {
        if let Some(target) = &self.target {
            match &mut discovery {
                Discovery::Git(value) => value.skills.retain(|skill| skill.name == *target),
                Discovery::Registry(value) => value.skills.retain(|skill| skill.name == *target),
                Discovery::Imported(value) => value.skills.retain(|skill| skill.name == *target),
            }
            if discovery.skills().len() != 1 {
                self.error = Some("skill_source_unresolved");
                return;
            }
        }
        self.known = known;
        if discovery.skills().len() == 1
            && (self.target.is_some()
                || !self
                    .known
                    .iter()
                    .any(|known| known.name == discovery.skills()[0].name))
        {
            self.selected.insert(0);
        }
        self.discovery = Some(discovery);
    }

    fn install(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed || self.pending {
            return;
        }
        if self.request.is_none() {
            if self.exceeds_capacity() {
                return;
            }
            let Some(index) = self
                .selected
                .iter()
                .find(|index| !self.completed.contains(index))
                .copied()
            else {
                return;
            };
            let Some(discovery) = &self.discovery else {
                return;
            };
            let candidate = &discovery.skills()[index];
            let expected_revision = self
                .known
                .iter()
                .find(|known| known.name == candidate.name)
                .map_or(0, |known| known.revision);
            if matches!(discovery, Discovery::Imported(_)) {
                let candidate = candidate.clone();
                self.prepare_install(index, candidate, expected_revision, window, cx);
                return;
            }
            let Some(command) = discovery.install(index, expected_revision) else {
                return;
            };
            self.request = Some((index, self.binding.client.prepare(command)));
        }
        let (index, request) = self.request.as_ref().unwrap().clone();
        let binding = self.binding.clone();
        let retry = cx.entity();
        let captured = request.clone();
        let ticket = crate::installations::track(
            binding.client.clone(),
            binding.runtime.clone(),
            &request,
            false,
            move |attempt, window, cx| {
                let show = retry.update(cx, |editor, cx| {
                    editor.closed = false;
                    if !editor.pending {
                        if matches!(attempt, crate::installations::Attempt::New)
                            && matches!(captured.command, Command::InstallSkillUpload { .. })
                        {
                            editor.request = None;
                            editor.lease = None;
                        } else {
                            editor.request =
                                Some((index, attempt.request(&editor.binding.client, &captured)));
                        }
                    }
                    !editor.dialog.presented(cx)
                });
                if show {
                    view::open(retry.clone(), window, cx);
                }
                retry.update(cx, |editor, cx| editor.install(window, cx));
            },
            window,
            cx,
        );
        self.pending = true;
        self.error = None;
        let job = binding
            .runtime
            .spawn(async move { binding.client.execute(request).await });
        let origin = window.window_handle();
        self.task = Some(cx.spawn(async move |editor, cx| {
            let result = job.await;
            if let Some(ticket) = &ticket {
                cx.update(|cx| ticket.record(crate::installations::result(&result), cx));
            }
            let Some(target) = cx.update(|cx| crate::installations::target(origin, cx)) else {
                let _ = editor.update(cx, |editor, cx| {
                    editor.pending = false;
                    if !crate::installations::result(&result)
                        .as_ref()
                        .is_err_and(uncertain)
                    {
                        editor.request = None;
                        editor.lease = None;
                    }
                    cx.notify();
                });
                return;
            };
            _ = target.update(cx, |_, window, cx| {
                editor.update(cx, |editor, cx| {
                    editor.pending = false;
                    match result {
                        Ok(Ok(Output::Plugin(info))) => {
                            editor.request = None;
                            editor.lease = None;
                            editor.completed.insert(index);
                            editor.known.retain(|known| known.name != info.summary.name);
                            editor.known.push(info.summary);
                            if !editor.closed
                                && editor
                                    .selected
                                    .iter()
                                    .any(|index| !editor.completed.contains(index))
                            {
                                editor.install(window, cx);
                            } else {
                                editor.closed = true;
                                editor.dialog.close(window, cx);
                            }
                        }
                        Ok(Err(error)) => {
                            if !uncertain(&error) {
                                editor.request = None;
                                editor.lease = None;
                            }
                            editor.error = Some(error_key(&error));
                        }
                        _ => editor.error = Some("plugins_unknown"),
                    }
                    cx.notify();
                })
            });
            if let Some(ticket) = &ticket {
                // The next selected request is now registered; close only when
                // the original sequence has actually finished.
                let _ = target.update(cx, |_, window, cx| ticket.close_empty(window, cx));
            }
        }));
        cx.notify();
    }

    fn capacity(&self) -> usize {
        let installed = self.known.len();
        sailry_protocol::plugin::MAX_INSTALLED.saturating_sub(installed)
    }

    fn exceeds_capacity(&self) -> bool {
        self.discovery.as_ref().is_some_and(|discovery| {
            self.selected
                .iter()
                .filter(|index| {
                    let candidate = &discovery.skills()[**index];
                    !self.known.iter().any(|known| known.name == candidate.name)
                })
                .count()
                > self.capacity()
        })
    }
}
