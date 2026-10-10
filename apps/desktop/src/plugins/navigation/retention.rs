//! The host owns view lifetimes; plugins only declare their navigation policy.
use super::*;
use sailry_protocol::plugin::desktop::Retention;
use std::time::Duration;

impl Navigation {
    fn release(&mut self, id: EntityId) {
        self.panels.retain(|(_, panel)| panel.entity_id() != id);
        if self
            .panel
            .as_ref()
            .is_some_and(|panel| panel.entity_id() == id)
        {
            self.panel = None;
            self.selected = None;
        }
        self.expiry.remove(&id);
    }
}

impl Shell {
    pub(crate) fn park_extension(&mut self, cx: &mut Context<Self>) {
        let Some(state) = &mut self.extensions else {
            return;
        };
        let Some(entry) = &state.selected else { return };
        let Some(panel) = &state.panel else { return };
        let id = panel.entity_id();
        match entry.navigation.retention {
            Retention::Always => {}
            Retention::None => state.release(id),
            Retention::Idle { seconds } => {
                let duration = Duration::from_secs(seconds.get().into());
                let task = cx.spawn(async move |owner, cx| {
                    cx.background_executor().timer(duration).await;
                    let _ = owner.update(cx, |shell, cx| {
                        let Some(state) = &mut shell.extensions else {
                            return;
                        };
                        if shell.page != Page::Plugin
                            || state
                                .panel
                                .as_ref()
                                .is_none_or(|panel| panel.entity_id() != id)
                        {
                            state.release(id);
                            cx.notify();
                        }
                    });
                });
                state.expiry.insert(id, task);
            }
        }
    }
}
