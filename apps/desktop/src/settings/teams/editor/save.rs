//! A team save writes only the team, with its exact revision. A failure keeps the draft;
//! an uncertain outcome keeps the request so Retry resends the same request ID.
use super::*;
use crate::settings::teams::{
    draft,
    requests::{self, Subject},
};
use gpui_kit::component::notification::Notification;
use sailry_protocol::{Command, Fault, Output};

/// The write for `team`, or None when it matches the stored team.
fn command(original: Option<&team::Profile>, team: team::Profile) -> Option<Command> {
    if original == Some(&team) {
        return None;
    }
    Some(Command::PutTeam {
        team,
        expected_revision: original.map_or(0, |team| team.revision),
    })
}

impl Editor {
    pub(super) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.builtin || self.pending {
            return;
        }
        let request = match self.request.clone() {
            Some(request) => request,
            None => {
                let team = match self.team(cx) {
                    Ok(team) => team,
                    Err(key) => {
                        self.error = Some(key);
                        cx.notify();
                        return;
                    }
                };
                let Some(command) = command(self.original.as_ref(), team) else {
                    window.close_dialog(cx);
                    return;
                };
                let request = self.catalog.binding.client.prepare(command);
                self.request = Some(request.clone());
                request
            }
        };
        self.pending = true;
        self.error = None;
        self.sync(cx);
        let job = requests::execute(&self.catalog.binding, request);
        cx.spawn_in(window, async move |editor, cx| {
            let result = job.await;
            _ = editor.update_in(cx, |editor, window, cx| editor.settle(result, window, cx));
        })
        .detach();
    }

    fn team(&self, cx: &App) -> Result<team::Profile, &'static str> {
        let name = self.name.read(cx).value().trim().to_owned();
        if name.is_empty() {
            return Err("team_invalid");
        }
        let key = match &self.original {
            Some(team) => team.key.clone(),
            None => draft::key(
                &name,
                "team",
                &self
                    .catalog
                    .teams
                    .iter()
                    .map(|team| team.key.clone())
                    .collect(),
            ),
        };
        Ok(team::Profile {
            key,
            name,
            appearance: self.appearance.clone(),
            description: self.description.read(cx).value().to_string(),
            model: self.lead.read(cx).value().cloned(),
            instructions: self.instructions.read(cx).value().to_string(),
            members: self.members.clone(),
            ..self.template.clone()
        })
    }

    fn settle(
        &mut self,
        result: Result<Result<Output, Fault>, tokio::task::JoinError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending = false;
        match result {
            Ok(Ok(Output::Team(_))) => {
                self.request = None;
                crate::feedback::toast(
                    window,
                    tr("team_saved"),
                    Notification::success(tr("team_saved")),
                    cx,
                );
                window.close_dialog(cx);
                return;
            }
            Ok(Err(fault)) => {
                if !requests::uncertain(fault.code) {
                    self.request = None;
                }
                self.error = Some(requests::key(Subject::Team, fault.code));
            }
            // The outcome is unknown, so the request stays for an explicit retry.
            _ => self.error = Some("role_unknown"),
        }
        self.sync(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    fn stored() -> team::Profile {
        team::Profile {
            id: TeamId::new(),
            revision: 7,
            key: "review".into(),
            name: "Review".into(),
            appearance: None,
            description: String::new(),
            model: None,
            instructions: String::new(),
            members: vec![RoleId::new(), RoleId::new()],
        }
    }

    #[test]
    fn skips_unchanged_team() {
        let team = stored();
        assert!(command(Some(&team), team.clone()).is_none());
    }

    #[test]
    fn sends_stored_revision() {
        let team = stored();
        let mut changed = team.clone();
        changed.members.reverse();
        let Some(Command::PutTeam {
            team: sent,
            expected_revision,
        }) = command(Some(&team), changed.clone())
        else {
            panic!("expected a team write");
        };
        assert_eq!(expected_revision, 7);
        assert_eq!(sent, changed);
    }

    #[test]
    fn creates_with_zero_revision() {
        let Some(Command::PutTeam {
            expected_revision, ..
        }) = command(None, stored())
        else {
            panic!("expected a team write");
        };
        assert_eq!(expected_revision, 0);
    }
}
