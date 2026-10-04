use super::model::ProjectRemoval;
use crate::{
    preview::Page,
    resources::{PreviewFiles, PreviewGit},
    shell::Shell,
    tr,
};
use gpui_kit::*;

impl Shell {
    pub(crate) fn remove_project_dialog(
        &self,
        id: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(expected) = self.workspace.project_removal(id) else {
            return;
        };
        let detail =
            rust_i18n::t!("project_remove_sample", name = expected.project.path).to_string();
        if let Some(error) = self.workspace.project_removal_reason(id) {
            crate::feedback::error(&tr("project_remove"), &tr(error), window, cx);
            return;
        }
        let shell = cx.entity().downgrade();
        let form = cx.new(|_| Confirmation { shell, expected });
        crate::prompts::confirm(
            &tr("project_remove"),
            &detail,
            tr("project_remove_preview"),
            window,
            cx,
            move |window, cx| form.update(cx, |form, cx| form.submit(window, cx)),
        );
    }
}

struct Confirmation {
    shell: WeakEntity<Shell>,
    expected: ProjectRemoval,
}

impl Confirmation {
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let result = self
            .shell
            .update(cx, |shell, cx| {
                let selected = shell
                    .workspace
                    .selected_owner(shell.host)
                    .is_some_and(|owner| owner.project == self.expected.id);
                shell.workspace.remove_project(&self.expected)?;
                shell.sidebar.open_project(self.expected.id);
                if selected {
                    shell.files = PreviewFiles::new(window, cx);
                    shell.git = PreviewGit::new(window, cx);
                    shell.close_resource_panel(cx);
                    if shell.page != Page::Settings {
                        let page = if shell.workspace.selected_owner(shell.host).is_some() {
                            Page::Project
                        } else {
                            Page::Host
                        };
                        shell.navigate(page, window, cx);
                    }
                }
                cx.notify();
                Ok::<_, &'static str>(())
            })
            .unwrap_or(Err("project_missing"));
        match result {
            Ok(()) => {}
            Err(error) => {
                crate::feedback::error(&tr("project_remove"), &tr(error), window, cx);
            }
        }
    }
}
