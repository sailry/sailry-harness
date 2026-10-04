use super::*;

impl View {
    pub(super) fn receive(&mut self, state: ScreenView, cx: &mut Context<Self>) {
        let previous = self
            .state
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.screen.features.clone());
        let connected = self.state.connected;
        let controlling = self.controlling();
        self.update_screen(state);
        let Some(snapshot) = &self.state.snapshot else {
            return;
        };
        let features = snapshot.screen.features.clone();
        if self.controlling() && self.focused && self.window_active {
            if features.focus_reporting
                && previous
                    .as_ref()
                    .is_some_and(|old| !old.focus_reporting || !controlling)
            {
                self.report_focus(true, cx);
            }
            // A restored snapshot describes state, not fresh OS side effects.
            if connected && let Some(previous) = previous {
                if let Some(clipboard) = features.clipboard
                    && previous.clipboard.as_ref().map(|old| old.sequence)
                        != Some(clipboard.sequence)
                {
                    cx.write_to_clipboard(ClipboardItem::new_string(clipboard.text));
                }
                if features.bell != previous.bell {
                    self.bell = true;
                    self.bell_reset = Some(cx.spawn(async move |view, cx| {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(150))
                            .await;
                        let _ = view.update(cx, |view, cx| {
                            view.bell = false;
                            cx.notify();
                        });
                    }));
                }
            }
        }
    }

    pub(super) fn report_focus(&mut self, focused: bool, cx: &mut Context<Self>) {
        if !self.controlling() {
            return;
        }
        let revision = self.state.snapshot.as_ref().unwrap().info.revision;
        self.send(
            Command::InputTerminal {
                terminal: self.binding.id,
                revision,
                input: protocol::Input::Focus {
                    focused: focused && self.window_active,
                },
            },
            cx,
        );
    }
}
