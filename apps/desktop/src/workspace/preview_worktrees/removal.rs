use crate::{
    resources::{PreviewFiles, PreviewGit},
    shell::Shell,
    tr,
    workspace::{Owner, model::Removal},
};
use gpui_kit::*;

impl Shell {
    pub(crate) fn remove_worktree_dialog(
        &self,
        owner: Owner,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(expected) = self.workspace.removal(owner) else {
            return;
        };
        let detail = rust_i18n::t!("worktree_remove_sample", name = expected.tree.path).to_string();
        if let Some(error) = self.workspace.removal_reason(owner) {
            crate::feedback::error(&tr("worktree_remove"), &tr(error), window, cx);
            return;
        }
        let shell = cx.entity().downgrade();
        let form = cx.new(|_| Confirmation { shell, expected });
        crate::prompts::confirm(
            &tr("worktree_remove"),
            &detail,
            tr("worktree_remove_preview"),
            window,
            cx,
            move |window, cx| form.update(cx, |form, cx| form.submit(window, cx)),
        );
    }
}

struct Confirmation {
    shell: WeakEntity<Shell>,
    expected: Removal,
}

impl Confirmation {
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let result = self
            .shell
            .update(cx, |shell, cx| {
                let selected =
                    shell.workspace.selected_owner(shell.host) == Some(self.expected.owner);
                shell.workspace.remove_worktree(&self.expected)?;
                if selected {
                    shell.files = PreviewFiles::new(window, cx);
                    shell.git = PreviewGit::new(window, cx);
                    shell.close_resource_panel(cx);
                }
                cx.notify();
                Ok::<_, &'static str>(())
            })
            .unwrap_or(Err("worktree_remove_missing"));
        match result {
            Ok(()) => {}
            Err(error) => {
                crate::feedback::error(&tr("worktree_remove"), &tr(error), window, cx);
            }
        }
    }
}
