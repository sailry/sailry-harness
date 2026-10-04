use super::model::Project;
use crate::theme::DialogStyle as _;
use crate::{preview::Page, shell::Shell, tr};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    form::field,
    input::{Input, InputState},
    scroll::ScrollableElement,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

mod appearance;
mod directory;
mod form;
mod host;
mod source;
#[cfg(test)]
mod tests;

const WIDTH: Pixels = px(560.);

impl Shell {
    pub(crate) fn project_editor(
        &self,
        host: usize,
        project: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<Editor>> {
        self.open_project_editor(host, project, None, window, cx)
    }

    pub(crate) fn clone_git_project(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(form) = self.project_editor(0, None, window, cx) {
            form.update(cx, |editor, cx| editor.select_source(true, window, cx));
        }
    }

    pub(crate) fn live_project_editor(
        &self,
        project: sailry_protocol::ProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<Editor>> {
        let original = self
            .live
            .as_ref()?
            .view
            .snapshot
            .as_ref()?
            .projects
            .iter()
            .find(|value| value.id == project)?
            .clone();
        self.open_project_editor(0, None, Some(original), window, cx)
    }

    fn open_project_editor(
        &self,
        host: usize,
        project: Option<usize>,
        live_original: Option<sailry_protocol::Project>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<Editor>> {
        let form = self.build_project_editor(host, project, live_original, window, cx)?;
        self.show_project_editor(form.clone(), window, cx);
        Some(form)
    }

    pub(crate) fn open_folder(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(form) = self.build_project_editor(self.host, None, None, window, cx) {
            directory::open_project(form, window, cx);
        }
    }

    fn build_project_editor(
        &self,
        host: usize,
        project: Option<usize>,
        live_original: Option<sailry_protocol::Project>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<Editor>> {
        let live = self.live.as_ref().map(|live| live.transport.clone());
        if live.is_some() && project.is_some() {
            return None;
        }
        let original = project.and_then(|id| {
            self.workspace
                .projects
                .get(&id)
                .cloned()
                .map(|value| (id, value))
        });
        if project.is_some()
            && original
                .as_ref()
                .is_none_or(|(_, value)| value.host != host)
        {
            return None;
        }
        let shell = cx.entity().downgrade();
        let form = cx.new(|cx| Editor {
            shell,
            host,
            host_bounds: Bounds::default(),
            page: if matches!(self.page, Page::Files | Page::Git)
                || (self.page == Page::Plugin && self.plugin_worktree())
            {
                self.page
            } else {
                Page::Project
            },
            inputs: [
                (
                    "project_name_placeholder",
                    original
                        .as_ref()
                        .map(|(_, value)| value.name.clone())
                        .or_else(|| {
                            live_original
                                .as_ref()
                                .map(|value| value.name.clone().into())
                        })
                        .unwrap_or_default(),
                ),
                (
                    "project_path_placeholder",
                    original
                        .as_ref()
                        .map(|(_, value)| value.path.clone())
                        .or_else(|| {
                            live_original
                                .as_ref()
                                .map(|value| value.path.clone().into())
                        })
                        .unwrap_or_default(),
                ),
            ]
            .map(|(placeholder, value)| {
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(value)
                        .placeholder(tr(placeholder))
                })
            }),
            appearance: original
                .as_ref()
                .map(|(_, project)| project.appearance.clone())
                .or_else(|| live_original.as_ref().map(|value| value.appearance.clone()))
                .unwrap_or_default(),
            original,
            live_original,
            clone: false,
            other_path: SharedString::default(),
            repository: cx.new(|cx| {
                InputState::new(window, cx).placeholder(tr("project_repository_placeholder"))
            }),
            branch: cx.new(|cx| {
                InputState::new(window, cx).placeholder(tr("project_branch_placeholder"))
            }),
            suggestion: String::new(),
            subscriptions: Vec::new(),
            error: None,
            live,
            pending: false,
            request: None,
            task: None,
        });
        form.update(cx, |editor, cx| editor.observe_source(window, cx));
        Some(form)
    }

    fn show_project_editor(
        &self,
        form: Entity<Editor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editing = form.read(cx).original.is_some() || form.read(cx).live_original.is_some();
        let is_live = form.read(cx).live.is_some();
        window.open_dialog(cx, move |dialog, window, cx| {
            let confirm = form.clone();
            let save = form.clone();
            dialog
                .margin_top(px(24.))
                .form_title(tr(if editing {
                    "project_edit"
                } else {
                    "project_add"
                }))
                .w((window.viewport_size().width - px(48.)).min(WIDTH))
                .overlay_closable(false)
                .on_ok(move |_, window, cx| {
                    confirm.update(cx, |editor, cx| editor.save(window, cx));
                    false
                })
                .child(form.clone())
                .footer(
                    gpui_kit::component::dialog::DialogFooter::new()
                        .w_full()
                        .gap_2()
                        .child(
                            Button::new("project-cancel")
                                .label(tr("settings_cancel"))
                                .debug_selector(|| "project-cancel".into())
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("project-save")
                                .primary()
                                .loading(form.read(cx).pending)
                                .when(form.read(cx).pending, |button| {
                                    button.icon(IconName::Loader)
                                })
                                .label(tr(if form.read(cx).clone {
                                    "project_clone"
                                } else if is_live {
                                    "settings_save"
                                } else {
                                    "settings_save_preview"
                                }))
                                .debug_selector(|| "project-save".into())
                                .on_click(move |_, window, cx| {
                                    save.update(cx, |editor, cx| editor.save(window, cx))
                                }),
                        ),
                )
        });
    }
}

pub(crate) struct Editor {
    shell: WeakEntity<Shell>,
    host: usize,
    host_bounds: Bounds<Pixels>,
    page: Page,
    original: Option<(usize, Project)>,
    live_original: Option<sailry_protocol::Project>,
    inputs: [Entity<InputState>; 2],
    clone: bool,
    other_path: SharedString,
    repository: Entity<InputState>,
    branch: Entity<InputState>,
    appearance: sailry_protocol::projects::Appearance,
    suggestion: String,
    subscriptions: Vec<Subscription>,
    error: Option<&'static str>,
    live: Option<std::sync::Arc<dyn sailry_link::Transport>>,
    pending: bool,
    request: Option<sailry_protocol::Request>,
    task: Option<Task<()>>,
}

impl Editor {
    fn open_registered(&self, path: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.request.is_some() {
            return false;
        }
        let opened = self
            .shell
            .update(cx, |shell, cx| {
                if let Some(transport) = &self.live {
                    let Some(live) = &mut shell.live else {
                        return false;
                    };
                    if live.selected != transport.target() {
                        return false;
                    }
                    let project = live
                        .view
                        .snapshot
                        .as_ref()
                        .and_then(|snapshot| {
                            snapshot
                                .projects
                                .iter()
                                .find(|project| project.path == path)
                        })
                        .map(|project| project.id);
                    let Some(project) = project else {
                        return false;
                    };
                    live.project = Some(project);
                    shell.navigate(self.page, window, cx);
                    true
                } else {
                    let project = shell
                        .workspace
                        .projects
                        .iter()
                        .find(|(_, project)| project.host == self.host && project.path == path)
                        .map(|(id, _)| *id);
                    let Some(project) = project else {
                        return false;
                    };
                    shell.select_project(project, window, cx);
                    if self.page != Page::Project {
                        shell.navigate(self.page, window, cx);
                    }
                    true
                }
            })
            .unwrap_or(false);
        if opened {
            window.close_dialog(cx);
        }
        opened
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        if self.original.is_none()
            && self.live_original.is_none()
            && self.request.is_none()
            && self.inputs[0].read(cx).value().trim().is_empty()
        {
            self.suggest(window, cx);
        }
        if self.live.is_some() {
            self.save_live(window, cx);
            return;
        }
        let name = self.inputs[0].read(cx).value();
        let path = match self.destination(cx) {
            Ok(path) => path,
            Err(error) => {
                self.fail(error, window, cx);
                return;
            }
        };
        let result = self
            .shell
            .update(cx, |shell, cx| {
                let id = shell.workspace.save_project(
                    self.host,
                    self.original.as_ref().map(|(id, value)| (*id, value)),
                    &name,
                    &path,
                )?;
                if let Some(project) = shell.workspace.projects.get_mut(&id) {
                    project.appearance = self.appearance.clone();
                    if self.clone {
                        project.repository = super::Repository::Ready;
                    }
                }
                shell.sidebar.open_project(id);
                shell.select_project(id, window, cx);
                if self.page != Page::Project {
                    shell.layout.panel_open[self.page.panel_index()] = true;
                    shell.navigate(self.page, window, cx);
                }
                Ok::<_, &'static str>(())
            })
            .unwrap_or(Err("project_missing"));
        match result {
            Ok(()) => window.close_dialog(cx),
            Err(error) => {
                self.fail(error, window, cx);
            }
        }
    }
}

impl Editor {
    fn save_live(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use sailry_client::Client;
        use sailry_protocol::{Command, Output};
        let transport = self.live.as_ref().unwrap().clone();
        let target = transport.target();
        let client = Client::new(transport);
        let name = self.inputs[0].read(cx).value().to_string();
        let path = match self.destination(cx) {
            Ok(path) => path,
            Err(error) => {
                self.fail(error, window, cx);
                return;
            }
        };
        let command = if let Some(expected) = &self.live_original {
            Command::UpdateProject {
                expected: expected.clone(),
                name,
                path,
                appearance: self.appearance.clone(),
            }
        } else {
            Command::CreateProject(sailry_protocol::projects::Draft {
                name,
                path,
                appearance: self.appearance.clone(),
                source: if self.clone {
                    let branch = self.branch.read(cx).value();
                    sailry_protocol::projects::Source::Clone {
                        url: self.repository.read(cx).value().trim().to_owned(),
                        branch: (!branch.trim().is_empty()).then(|| branch.trim().to_owned()),
                    }
                } else {
                    sailry_protocol::projects::Source::Local
                },
            })
        };
        if self
            .request
            .as_ref()
            .is_some_and(|request| request.command != command)
        {
            self.fail("project_outcome_unknown", window, cx);
            return;
        }
        let request = self
            .request
            .get_or_insert_with(|| client.prepare(command))
            .clone();
        let runtime = cx.global::<crate::backend::Services>().runtime.clone();
        let job = runtime.spawn(async move { client.execute(request).await });
        self.pending = true;
        self.error = None;
        self.task = Some(cx.spawn_in(window, async move |editor, cx| {
            let result = job.await;
            let _ = editor.update_in(cx, |editor, window, cx| {
                editor.pending = false;
                if let Ok(Ok(Output::Project(project))) = result {
                    let _ = editor.shell.update(cx, |shell, cx| {
                        if let Some(live) = &mut shell.live
                            && live.selected == target
                        {
                            live.project = Some(project.id);
                            if matches!(editor.page, Page::Files | Page::Git) {
                                shell.layout.panel_open[editor.page.panel_index()] = true;
                            }
                            shell.navigate(editor.page, window, cx);
                        }
                    });
                    window.close_dialog(cx);
                } else {
                    let uncertain = !matches!(&result, Ok(Err(error)) if !matches!(error.code,
                        sailry_protocol::ErrorCode::OutcomeUnknown | sailry_protocol::ErrorCode::Unavailable));
                    if !uncertain { editor.request = None; }
                    editor.fail(
                        if uncertain {
                            "project_outcome_unknown"
                        } else if matches!(&result, Ok(Err(error)) if error.code == sailry_protocol::ErrorCode::RevisionConflict) {
                            "project_changed"
                        } else if matches!(&result, Ok(Err(error)) if error.code == sailry_protocol::ErrorCode::Busy) {
                            "project_path_in_use"
                        } else if editor.clone {
                            "project_clone_failed"
                        } else {
                            "live_request_failed"
                        },
                        window,
                        cx,
                    );
                }
            });
        }));
        cx.notify();
    }
}
