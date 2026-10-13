//! Project inventory removal keeps execution history and files on the Node.
use super::*;
use sailry_protocol::{ErrorCode, Fault, FaultDetails, Project, Request};
#[cfg(test)]
mod tests;

impl Shell {
    pub(super) fn remove_live_project(
        &self,
        id: ProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(live) = &self.live else { return };
        let Some(expected) = live
            .view
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.projects.iter().find(|project| project.id == id))
            .cloned()
        else {
            return;
        };
        let owner = cx.entity().downgrade();
        let client = Arc::new(Client::new(live.transport.clone()));
        let request = client.prepare(Command::RemoveProject {
            expected: expected.clone(),
        });
        let form = cx.new(|_| Removal {
            owner,
            client,
            expected,
            request,
            pending: false,
            error: None,
        });
        form.update(cx, |form, cx| form.prompt(window, cx));
    }
}

struct Removal {
    owner: WeakEntity<Shell>,
    client: Arc<Client>,
    expected: Project,
    request: Request,
    pending: bool,
    error: Option<&'static str>,
}

impl Removal {
    fn prompt(&self, window: &mut Window, cx: &mut Context<Self>) {
        let detail = rust_i18n::t!("project_remove_named", name = self.expected.path).to_string();
        let form = cx.entity();
        crate::prompts::confirm(
            &tr("project_remove"),
            &detail,
            tr("project_remove"),
            window,
            cx,
            move |window, cx| form.update(cx, |form, cx| form.submit(window, cx)),
        );
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        let client = self.client.clone();
        let request = self.request.clone();
        let job = cx
            .global::<Services>()
            .runtime
            .spawn(async move { client.execute(request).await });
        self.pending = true;
        self.error = None;
        let form = cx.entity();
        cx.spawn_in(window, async move |_, cx| {
            let result = job.await;
            let _ = form.update_in(cx, |form, window, cx| {
                form.pending = false;
                match result {
                    Ok(Ok(Output::ProjectRemoved { id })) if id == form.expected.id => {
                        use gpui_kit::component::notification::Notification;
                        crate::feedback::toast(
                            window,
                            tr("project_removed"),
                            Notification::success(tr("project_removed"))
                                .id1::<Self>(("removed", cx.entity_id())),
                            cx,
                        );
                        let _ = form.owner.update(cx, |shell, cx| {
                            if let Some(live) = &mut shell.live
                                && live.selected == form.request.target
                                && live.project == Some(id)
                            {
                                live.project = None;
                                shell.navigate(crate::preview::Page::Host, window, cx);
                            }
                        });
                    }
                    Ok(Err(error)) => {
                        let (key, uncertain) = failure(&error);
                        form.error = Some(key);
                        // Definitive failures are durable too; a later retry is a new attempt.
                        if !uncertain {
                            form.request = form.client.prepare(Command::RemoveProject {
                                expected: form.expected.clone(),
                            });
                        }
                    }
                    _ => form.error = Some("project_remove_unknown"),
                }
                if let Some(error) = form.error {
                    use gpui_kit::component::notification::Notification;
                    crate::feedback::toast(
                        window,
                        tr(error),
                        Notification::error(tr(error)).id1::<Self>(("error", cx.entity_id())),
                        cx,
                    );
                    form.prompt(window, cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

fn failure(error: &Fault) -> (&'static str, bool) {
    let uncertain = error.code == ErrorCode::OutcomeUnknown
        || (error.code == ErrorCode::Unavailable && error.details.is_none());
    if uncertain {
        return ("project_remove_unknown", true);
    }
    if let Some(FaultDetails::ProjectRemoval {
        tasks_active,
        terminals_may_be_closed,
    }) = &error.details
    {
        return (
            match (*tasks_active, *terminals_may_be_closed, error.code) {
                (true, true, _) => "project_remove_tasks_partial",
                (true, false, _) => "project_remove_tasks",
                (false, true, ErrorCode::RevisionConflict) => "project_remove_changed_partial",
                (false, true, _) => "project_remove_failed",
                _ => "live_request_failed",
            },
            false,
        );
    }
    (
        match error.code {
            ErrorCode::Busy => "project_remove_busy",
            ErrorCode::RevisionConflict | ErrorCode::NotFound => "project_changed",
            _ => "live_request_failed",
        },
        false,
    )
}
