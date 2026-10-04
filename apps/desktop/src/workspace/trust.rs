use super::Owner;
use crate::{shell::Shell, tr};
use gpui_kit::*;

#[derive(Clone, Copy)]
pub(crate) enum Pending {
    NewSession(Owner),
    LaunchCli(Owner),
    CliProfile(Owner, &'static str),
    Send((usize, usize)),
}

impl Shell {
    pub(crate) fn project_trust(
        &self,
        project: usize,
        pending: Option<Pending>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(record) = self.workspace.projects.get(&project) else {
            return;
        };
        let trusted = record.trusted;
        let name = record.name.clone();
        let path = record.path.clone();
        let shell = cx.entity().downgrade();
        crate::prompts::confirm(
            &tr(if trusted {
                "project_revoke_title"
            } else {
                "project_grant_title"
            }),
            &format!(
                "{name}\n{path}\n{}\n{}",
                tr(if trusted {
                    "project_revoke_description"
                } else {
                    "project_grant_description"
                }),
                tr("project_trust_preview")
            ),
            tr(if trusted {
                "project_revoke"
            } else {
                "project_grant"
            }),
            window,
            cx,
            move |window, cx| {
                _ = shell.update(cx, |shell, cx| {
                    let Some(record) = shell.workspace.projects.get_mut(&project) else {
                        return;
                    };
                    if record.path != path || record.trusted != trusted {
                        return;
                    }
                    record.trusted = !trusted;
                    if trusted {
                        let keys: Vec<_> = shell
                            .workspace
                            .sessions
                            .iter()
                            .filter(|(_, session)| session.owner.project == project)
                            .map(|(&key, _)| key)
                            .collect();
                        for key in keys {
                            shell.stop_preview(key, cx);
                        }
                    } else if let Some(pending) = pending {
                        match pending {
                            Pending::CliProfile(owner, profile) => shell.start_preview_cli(
                                &super::cli::menu::Start { owner, profile },
                                window,
                                cx,
                            ),
                            Pending::LaunchCli(owner) => shell.cli_launcher(owner, window, cx),
                            Pending::NewSession(owner) => shell.create_session(owner, window, cx),
                            Pending::Send(key) => {
                                if shell
                                    .workspace
                                    .sessions
                                    .get(&key)
                                    .is_some_and(|session| session.owner.project == project)
                                {
                                    shell.send_preview(key, window, cx)
                                }
                            }
                        }
                    }
                    cx.notify();
                });
            },
        );
    }
}
