use super::*;
use gpui_kit::component::input::{InputEvent, NumberInput, NumberInputEvent, StepAction};

pub(super) fn input(window: &mut Window, cx: &mut Context<Host>) -> Entity<InputState> {
    let input = cx.new(|cx| {
        let mut input = InputState::new(window, cx)
            .default_value(Settings::default().child_concurrency.to_string())
            .placeholder(tr("child_concurrency_title"));
        // Keep the full integer domain instead of stepping through floating-point values.
        input.set_step(None, window, cx);
        input
    });
    cx.subscribe_in(
        &input,
        window,
        |host, _, event: &InputEvent, window, cx| match event {
            InputEvent::PressEnter { .. } => host.save_capacity(window, cx),
            InputEvent::Change => cx.notify(),
            _ => {}
        },
    )
    .detach();
    cx.subscribe_in(
        &input,
        window,
        |host, input, event: &NumberInputEvent, window, cx| {
            if host.locked() {
                return;
            }
            let NumberInputEvent::Step(action) = event;
            if let Some(value) = stepped(input.read(cx).value().as_ref(), *action) {
                input.update(cx, |input, cx| {
                    input.set_value(value.to_string(), window, cx)
                });
                cx.notify();
            }
        },
    )
    .detach();
    input
}

impl Host {
    pub(super) fn locked(&self) -> bool {
        self.pending || self.saved.is_none() || self.error == Some("background_settings_unknown")
    }

    pub(super) fn capacity(&self, cx: &Context<Self>) -> Row {
        let unchanged = self.saved.as_ref().is_some_and(|saved| {
            count(self.concurrency.read(cx).value().as_ref()) == Some(saved.child_concurrency)
        });
        Row::new(
            "child_concurrency_title",
            h_flex()
                .gap_2()
                .child(
                    NumberInput::new(&self.concurrency)
                        .w_40()
                        .disabled(self.locked()),
                )
                .child(
                    Button::new("child-concurrency-save")
                        .label(tr("settings_save"))
                        .disabled(self.locked() || unchanged)
                        .on_click(
                            cx.listener(|host, _, window, cx| host.save_capacity(window, cx)),
                        ),
                ),
        )
    }

    fn save_capacity(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        let Some(settings) = self
            .saved
            .as_ref()
            .and_then(|saved| configured(saved, self.concurrency.read(cx).value().as_ref()))
        else {
            window.push_notification(
                notification::Notification::error(tr("child_concurrency_invalid")),
                cx,
            );
            return;
        };
        if self.saved.as_ref() != Some(&settings) {
            self.persist(settings, window, cx);
        }
    }

    pub(super) fn sync_capacity(
        &mut self,
        settings: &Settings,
        submitted: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = self.concurrency.read(cx).value();
        if replace(self.saved.as_ref(), value.as_ref(), submitted) {
            self.concurrency.update(cx, |input, cx| {
                input.set_value(settings.child_concurrency.to_string(), window, cx)
            });
        }
    }
}

fn count(value: &str) -> Option<u64> {
    value.trim().parse::<u64>().ok().filter(|value| *value > 0)
}

fn configured(saved: &Settings, value: &str) -> Option<Settings> {
    Some(Settings {
        child_concurrency: count(value)?,
        ..saved.clone()
    })
}

fn stepped(value: &str, action: StepAction) -> Option<u64> {
    let value = count(value)?;
    match action {
        StepAction::Increment => value.checked_add(1),
        StepAction::Decrement => value.checked_sub(1).filter(|value| *value > 0),
    }
}

fn replace(saved: Option<&Settings>, value: &str, submitted: Option<u64>) -> bool {
    let Some(saved) = saved else { return true };
    let value = count(value);
    value == Some(saved.child_concurrency) || value.is_some_and(|value| submitted == Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn preserves_integer_precision_and_other_settings() {
        let saved = Settings {
            revision: 7,
            resume: false,
            ..Default::default()
        };
        let value = configured(&saved, "18446744073709551615").unwrap();
        assert_eq!(value.child_concurrency, u64::MAX);
        assert_eq!(value.revision, 7);
        assert!(!value.resume);
        for value in ["", "0", "-1", "1.5", "1e3", "18446744073709551616"] {
            assert!(configured(&saved, value).is_none(), "{value}");
        }
        assert_eq!(
            stepped("9007199254740992", StepAction::Increment),
            Some(9007199254740993)
        );
        assert_eq!(stepped("1", StepAction::Decrement), None);
        assert_eq!(stepped(&u64::MAX.to_string(), StepAction::Increment), None);
    }

    #[test]
    fn keeps_drafts_across_unrelated_saves() {
        let saved = Settings::default();
        assert!(!replace(Some(&saved), "16", Some(saved.child_concurrency)));
        assert!(!replace(Some(&saved), "invalid", None));
        assert!(replace(Some(&saved), "16", Some(16)));
        assert!(replace(
            Some(&saved),
            &saved.child_concurrency.to_string(),
            None
        ));
        assert!(replace(None, "8", None));
    }
}
