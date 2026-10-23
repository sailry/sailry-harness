use super::*;
use std::collections::BTreeMap;

pub(super) type Key = (sailry_protocol::NodeId, String);

pub(super) struct Action {
    pub(super) binding: Binding,
    request: Request,
    pub(super) pending: bool,
}

fn admit(actions: &mut BTreeMap<Key, Action>, binding: Binding, command: Command) -> Option<Key> {
    let name = match &command {
        Command::SetPluginEnabled { name, .. } | Command::RemovePlugin { name, .. } => name.clone(),
        _ => return None,
    };
    let key = (binding.client.target(), name);
    let std::collections::btree_map::Entry::Vacant(entry) = actions.entry(key.clone()) else {
        return None;
    };
    entry.insert(Action {
        request: binding.client.prepare(command),
        binding,
        pending: false,
    });
    Some(key)
}

fn start(actions: &mut BTreeMap<Key, Action>, key: &Key) -> Option<(Binding, Request)> {
    let action = actions.get_mut(key)?;
    if action.pending {
        return None;
    }
    action.pending = true;
    Some((action.binding.clone(), action.request.clone()))
}

impl Workspace {
    pub(super) fn skill_action(
        &mut self,
        binding: Binding,
        command: Command,
        cx: &mut Context<Self>,
    ) {
        if let Some(key) = admit(&mut self.skill_state.actions, binding, command) {
            self.send_skill_action(&key, cx);
        }
    }

    pub(super) fn send_skill_action(&mut self, key: &Key, cx: &mut Context<Self>) {
        let Some((binding, request)) = start(&mut self.skill_state.actions, key) else {
            return;
        };
        let key = key.clone();
        self.skill_state.error = None;
        let id = request.id;
        let job = binding
            .runtime
            .spawn(async move { binding.client.execute(request).await });
        cx.spawn(async move |owner, cx| {
            let result = job.await;
            _ = owner.update(cx, |owner, cx| {
                let state = &mut owner.skill_state;
                let Some(action) = state.actions.get_mut(&key) else {
                    return;
                };
                if action.request.id != id {
                    return;
                }
                action.pending = false;
                match result {
                    Ok(Ok(Output::Plugin(_) | Output::Plugins(_))) => {
                        state.actions.remove(&key);
                        state.error = None;
                    }
                    Ok(Err(error)) => {
                        if !uncertain(&error) {
                            state.actions.remove(&key);
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

#[cfg(test)]
mod tests;
