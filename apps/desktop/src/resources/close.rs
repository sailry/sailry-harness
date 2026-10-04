//! Keyboard closure is scoped by the focused pane or inspector subtree.
use super::*;
use crate::{panes::Target, shell::shortcuts::CloseFocused};

impl Shell {
    pub(crate) fn close_pane(
        &mut self,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let members = self.splits.read(cx).ordered_members(target, cx);
        if members.len() > 1 {
            let next = self
                .splits
                .read(cx)
                .active
                .filter(|active| *active != target && members.contains(active))
                .or_else(|| members.iter().copied().find(|member| *member != target))
                .unwrap();
            self.splits.update(cx, |splits, cx| {
                splits.detach(target, window, cx);
                splits.focus(next, cx);
            });
            self.activate_pane(next, window, cx);
        } else if let Target::Terminal(node, tree, terminal) = target {
            self.close_terminal_pane(node, tree, terminal, window, cx);
        } else {
            self.remove_pane(target, window, cx);
        }
    }

    pub(crate) fn confirm_close_pane(
        &mut self,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(target, Target::Terminal(..))
            || self.splits.read(cx).ordered_members(target, cx).len() > 1
        {
            self.close_pane(target, window, cx);
            return;
        }
        if crate::prompts::active(window, cx) || !self.splits.read(cx).contains(target) {
            return;
        }
        let response = crate::prompts::ask(
            PromptLevel::Info,
            &tr("pane_close_confirm"),
            &tr("pane_close_description"),
            &[tr("settings_cancel"), tr("close")],
            window,
            cx,
        );
        cx.spawn_in(window, async move |shell, cx| {
            if response.await == Some(1) {
                let _ = shell.update_in(cx, |shell, window, cx| {
                    if shell.splits.read(cx).contains(target) {
                        shell.remove_pane(target, window, cx);
                    }
                });
            }
        })
        .detach();
    }

    pub(crate) fn close_focused_resource(
        &mut self,
        _: &CloseFocused,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match &self.side_resource {
            Some(SideResource::Child(panel)) => {
                self.close_live_child(panel.selected, window, cx);
            }
            Some(SideResource::PreviewFiles(state)) if !state.tabs.open.is_empty() => {
                self.close_file_tab(true, state.tabs.selected, window, cx);
                if matches!(&self.side_resource, Some(SideResource::PreviewFiles(state)) if state.tabs.open.is_empty())
                {
                    self.close_resource_panel(cx);
                }
            }
            Some(SideResource::PreviewGit(state)) if !state.tabs.open.is_empty() => {
                self.close_git_tab(true, state.tabs.selected, window, cx);
                if matches!(&self.side_resource, Some(SideResource::PreviewGit(state)) if state.tabs.open.is_empty())
                {
                    self.panel_focus.focus(window, cx);
                }
            }
            Some(panel) if panel.browser(cx).is_some() => {
                panel
                    .browser(cx)
                    .unwrap()
                    .update(cx, |browser, cx| browser.close_selected(window, cx));
            }
            _ => self.close_file_panel(window, cx),
        }
        if self.side_resource.is_none() {
            self.focus.focus(window, cx);
        }
    }
}
