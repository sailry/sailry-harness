//! A team save writes changed and new roles first, then the team, each with its exact
//! revision. A failure stops the sequence and keeps the draft; written roles adopt their
//! new revisions, so the next Save continues where the previous one stopped.
use super::*;
use crate::settings::teams::{
    draft,
    requests::{self, Subject},
};
use gpui_kit::component::notification::Notification;
use sailry_protocol::{Command, Fault, Output};

enum Next {
    Write(Step, Command),
    Done,
}

impl Editor {
    pub(super) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.builtin || self.pending {
            return;
        }
        if let Some((step, request)) = self.request.clone() {
            self.execute(step, request, window, cx);
            return;
        }
        if let Err(key) = self.check(cx) {
            self.fail(key, cx);
            return;
        }
        self.saved.clear();
        self.advance(window, cx);
    }

    fn fail(&mut self, key: &'static str, cx: &mut Context<Self>) {
        self.error = Some(key);
        cx.notify();
    }

    /// Validates every write before the first request is sent.
    fn check(&self, cx: &App) -> Result<(), &'static str> {
        if self.name.read(cx).value().trim().is_empty() {
            return Err("team_invalid");
        }
        for member in &self.members {
            if let Some(form) = &member.form {
                let key = member
                    .original
                    .as_ref()
                    .map_or_else(|| "role".to_owned(), |role| role.key.clone());
                form.read(cx).profile(Some(key), cx)?;
            }
        }
        Ok(())
    }

    fn team(&self, cx: &App) -> team::Profile {
        let name = self.name.read(cx).value().trim().to_owned();
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
        team::Profile {
            key,
            name,
            appearance: self.appearance.clone(),
            description: self.description.read(cx).value().to_string(),
            model: self.lead.read(cx).value().cloned(),
            instructions: self.instructions.read(cx).value().to_string(),
            members: self.members.iter().map(|member| member.id).collect(),
            ..self.template.clone()
        }
    }

    fn next(&self, cx: &App) -> Result<Next, &'static str> {
        let taken: BTreeSet<String> = self
            .catalog
            .roles
            .iter()
            .map(|role| role.key.clone())
            .chain(
                self.members
                    .iter()
                    .filter_map(|member| member.original.as_ref().map(|role| role.key.clone())),
            )
            .collect();
        for member in &self.members {
            let Some(form) = member
                .form
                .as_ref()
                .filter(|_| !self.saved.contains(&member.id))
            else {
                continue;
            };
            let form = form.read(cx);
            let key = match &member.original {
                Some(role) => role.key.clone(),
                None => draft::key(&form.name(cx), "role", &taken),
            };
            let role = form.profile(Some(key), cx)?;
            if member.original.as_ref() != Some(&role) {
                let expected_revision = member.original.as_ref().map_or(0, |role| role.revision);
                return Ok(Next::Write(
                    Step::Role(member.id),
                    Command::PutRole {
                        role,
                        expected_revision,
                    },
                ));
            }
        }
        let team = self.team(cx);
        if self.original.as_ref() == Some(&team) {
            return Ok(Next::Done);
        }
        let expected_revision = self.original.as_ref().map_or(0, |team| team.revision);
        Ok(Next::Write(
            Step::Team,
            Command::PutTeam {
                team,
                expected_revision,
            },
        ))
    }

    fn advance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.next(cx) {
            Err(key) => self.fail(key, cx),
            Ok(Next::Done) => self.finish(!self.saved.is_empty(), window, cx),
            Ok(Next::Write(step, command)) => {
                let request = self.catalog.binding.client.prepare(command);
                self.request = Some((step, request.clone()));
                self.execute(step, request, window, cx);
            }
        }
    }

    fn execute(
        &mut self,
        step: Step,
        request: Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending = true;
        self.error = None;
        self.sync(cx);
        let job = requests::execute(&self.catalog.binding, request);
        cx.spawn_in(window, async move |editor, cx| {
            let result = job.await;
            _ = editor.update_in(cx, |editor, window, cx| {
                editor.settle(step, result, window, cx)
            });
        })
        .detach();
    }

    fn settle(
        &mut self,
        step: Step,
        result: Result<Result<Output, Fault>, tokio::task::JoinError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending = false;
        match (step, result) {
            (Step::Role(id), Ok(Ok(Output::Role(role)))) => {
                self.request = None;
                self.saved.insert(id);
                if let Some(member) = self.members.iter_mut().find(|member| member.id == id) {
                    member.original = Some(role.clone());
                    if let Some(form) = &member.form {
                        form.update(cx, |form, _| form.accept(role));
                    }
                }
                self.sync(cx);
                self.advance(window, cx);
            }
            (Step::Team, Ok(Ok(Output::Team(team)))) => {
                self.request = None;
                self.original = Some(team);
                self.sync(cx);
                self.finish(true, window, cx);
            }
            (step, Ok(Err(fault))) => {
                if !requests::uncertain(fault.code) {
                    self.request = None;
                }
                let subject = match step {
                    Step::Role(_) => Subject::Role,
                    Step::Team => Subject::Team,
                };
                self.error = Some(requests::key(subject, fault.code));
                self.sync(cx);
            }
            _ => {
                self.error = Some("role_unknown");
                self.sync(cx);
            }
        }
    }

    fn finish(&mut self, wrote: bool, window: &mut Window, cx: &mut Context<Self>) {
        if wrote {
            crate::feedback::toast(
                window,
                tr("team_saved"),
                Notification::success(tr("team_saved")),
                cx,
            );
        }
        window.close_dialog(cx);
    }
}
