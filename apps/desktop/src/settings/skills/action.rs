use super::*;

pub(super) struct Action {
    pub(super) binding: Binding,
    request: Request,
    pub(super) pending: bool,
}

impl Workspace {
    pub(super) fn skill_action(
        &mut self,
        binding: Binding,
        command: Command,
        cx: &mut Context<Self>,
    ) {
        if self.skill_state.action.is_some() {
            return;
        }
        self.skill_state.action = Some(Action {
            request: binding.client.prepare(command),
            binding,
            pending: false,
        });
        self.send_skill_action(cx);
    }

    pub(super) fn send_skill_action(&mut self, cx: &mut Context<Self>) {
        let Some(action) = &mut self.skill_state.action else {
            return;
        };
        if action.pending {
            return;
        }
        action.pending = true;
        self.skill_state.error = None;
        let binding = action.binding.clone();
        let request = action.request.clone();
        let id = request.id;
        let job = binding
            .runtime
            .spawn(async move { binding.client.execute(request).await });
        cx.spawn(async move |owner, cx| {
            let result = job.await;
            _ = owner.update(cx, |owner, cx| {
                let state = &mut owner.skill_state;
                let Some(action) = &mut state.action else {
                    return;
                };
                if action.request.id != id {
                    return;
                }
                action.pending = false;
                match result {
                    Ok(Ok(Output::Plugin(_) | Output::Plugins(_))) => {
                        state.action = None;
                        state.error = None;
                    }
                    Ok(Err(error)) => {
                        if !uncertain(&error) {
                            state.action = None;
                        }
                        state.error = Some(error_key(&error));
                    }
                    _ => state.error = Some("plugins_unknown"),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
