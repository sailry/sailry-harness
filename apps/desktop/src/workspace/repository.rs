use super::{Repository, model::Project};
use crate::{resources::SideResource, shell::Shell, tr};
use gpui_kit::*;

impl Shell {
    pub(crate) fn initialize_git(
        &self,
        project: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(expected) = self.workspace.projects.get(&project).cloned() else {
            return;
        };
        if expected.repository != Repository::Directory {
            return;
        }
        let detail = rust_i18n::t!("git_initialize_sample", name = expected.path).to_string();
        let shell = cx.entity().downgrade();
        let form = cx.new(|_| Confirmation {
            shell,
            project,
            expected,
        });
        crate::prompts::confirm(
            &tr("git_initialize"),
            &detail,
            tr("git_initialize_preview"),
            window,
            cx,
            move |window, cx| form.update(cx, |form, cx| form.submit(window, cx)),
        );
    }
}

struct Confirmation {
    shell: WeakEntity<Shell>,
    project: usize,
    expected: Project,
}

impl Confirmation {
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let result = self
            .shell
            .update(cx, |shell, cx| {
                let initializing = shell
                    .workspace
                    .projects
                    .get(&self.project)
                    .is_some_and(|project| project.repository == Repository::Directory);
                shell
                    .workspace
                    .initialize_repository(self.project, &self.expected)?;
                if initializing
                    && shell
                        .workspace
                        .selected_owner(shell.host)
                        .is_some_and(|owner| owner.project == self.project)
                {
                    shell.git.tabs.open.clear();
                    if let Some(SideResource::PreviewGit(state)) = &mut shell.side_resource {
                        state.tabs.open.clear();
                    }
                }
                cx.notify();
                Ok::<_, &'static str>(())
            })
            .unwrap_or(Err("project_missing"));
        match result {
            Ok(()) => {}
            Err(error) => {
                crate::feedback::error(&tr("git_initialize"), &tr(error), window, cx);
            }
        }
    }
}
