use super::*;
use crate::theme::DialogStyle as _;
use gpui_kit::component::{
    checkbox::Checkbox, dialog::DialogFooter, form::Field, scroll::ScrollableElement,
};
use sailry_protocol::plugin::{
    Summary,
    skills::{Discovery, Source},
};
use std::collections::BTreeSet;

mod view;

struct Editor {
    binding: Binding,
    known: Vec<Summary>,
    fields: [Entity<InputState>; 3],
    discovery: Option<Discovery>,
    selected: BTreeSet<usize>,
    completed: BTreeSet<usize>,
    request: Option<(usize, Request)>,
    pending: bool,
    error: Option<&'static str>,
    closed: bool,
    task: Option<Task<()>>,
}

pub(super) fn open(binding: Binding, known: Vec<Summary>, window: &mut Window, cx: &mut App) {
    let editor = cx.new(|cx| {
        let fields = [
            "skills_repository_hint",
            "skills_revision_hint",
            "skills_path_hint",
        ]
        .map(|key| cx.new(|cx| InputState::new(window, cx).placeholder(tr(key))));
        for input in &fields {
            cx.subscribe(input, |editor: &mut Editor, _, event, cx| {
                if matches!(event, InputEvent::Change)
                    && !editor.pending
                    && editor.request.is_none()
                {
                    editor.discovery = None;
                    editor.selected.clear();
                    editor.completed.clear();
                    editor.error = None;
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
            fields,
            discovery: None,
            selected: BTreeSet::new(),
            completed: BTreeSet::new(),
            request: None,
            pending: false,
            error: None,
            closed: false,
            task: None,
        }
    });
    view::open(editor, window, cx);
}

impl Editor {
    fn discover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed || self.pending || self.request.is_some() {
            return;
        }
        let values = self
            .fields
            .each_ref()
            .map(|field| field.read(cx).value().trim().to_string());
        if values[0].is_empty() {
            self.error = Some("skills_source_invalid");
            cx.notify();
            return;
        }
        let source = Source {
            repository: values[0].clone(),
            git_ref: (!values[1].is_empty()).then(|| values[1].clone()),
            path: (!values[2].is_empty()).then(|| values[2].clone()),
        };
        self.pending = true;
        self.error = None;
        self.discovery = None;
        self.selected.clear();
        self.completed.clear();
        let binding = self.binding.clone();
        let job = binding.runtime.spawn(async move {
            let discovery = binding
                .client
                .execute(binding.client.prepare(Command::DiscoverSkills { source }))
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
                editor.pending = false;
                if editor.closed {
                    return;
                }
                match result {
                    Ok(Ok((Output::SkillDiscovery(discovery), Output::Plugins(known)))) => {
                        editor.known = known;
                        if discovery.skills.len() == 1
                            && !editor
                                .known
                                .iter()
                                .any(|known| known.name == discovery.skills[0].name)
                        {
                            editor.selected.insert(0);
                        }
                        editor.discovery = Some(discovery);
                    }
                    Ok(Err(error)) => editor.error = Some(error_key(&error)),
                    _ => editor.error = Some("skills_failed"),
                }
                cx.notify();
            });
        }));
        cx.notify();
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
            let candidate = &discovery.skills[index];
            let command = Command::InstallSkill {
                source: discovery.source.clone(),
                path: candidate.path.clone(),
                name: candidate.name.clone(),
                expected_revision: self
                    .known
                    .iter()
                    .find(|known| known.name == candidate.name)
                    .map_or(0, |known| known.revision),
            };
            self.request = Some((index, self.binding.client.prepare(command)));
        }
        let (index, request) = self.request.as_ref().unwrap().clone();
        let binding = self.binding.clone();
        self.pending = true;
        self.error = None;
        let job = binding
            .runtime
            .spawn(async move { binding.client.execute(request).await });
        self.task = Some(cx.spawn_in(window, async move |editor, cx| {
            let result = job.await;
            _ = editor.update_in(cx, |editor, window, cx| {
                editor.pending = false;
                if editor.closed {
                    return;
                }
                match result {
                    Ok(Ok(Output::Plugin(info))) => {
                        editor.request = None;
                        editor.completed.insert(index);
                        editor.known.retain(|known| known.name != info.summary.name);
                        editor.known.push(info.summary);
                        if editor
                            .selected
                            .iter()
                            .any(|index| !editor.completed.contains(index))
                        {
                            editor.install(window, cx);
                        } else {
                            editor.closed = true;
                            window.close_dialog(cx);
                        }
                    }
                    Ok(Err(error)) => {
                        if !uncertain(&error) {
                            editor.request = None;
                        }
                        editor.error = Some(error_key(&error));
                    }
                    _ => editor.error = Some("plugins_unknown"),
                }
                cx.notify();
            });
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
                    let candidate = &discovery.skills[**index];
                    !self.known.iter().any(|known| known.name == candidate.name)
                })
                .count()
                > self.capacity()
        })
    }
}
