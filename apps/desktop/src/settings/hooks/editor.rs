use super::*;
use crate::{theme::DialogStyle as _, tr};
use gpui_kit::component::{
    button::Button,
    dialog::DialogFooter,
    form::Field,
    input::{Input, InputEvent, InputState},
    searchable_list::SearchableListItem,
    select::{Select, SelectEvent, SelectState},
};
use std::time::Duration;

#[derive(Clone)]
struct Choice(Phase);
impl SearchableListItem for Choice {
    type Value = Phase;
    fn title(&self) -> SharedString {
        tr(super::view::key(self.0))
    }
    fn value(&self) -> &Phase {
        &self.0
    }
}

pub(super) struct Editor {
    owner: Entity<Panel>,
    id: String,
    previous: Option<(Phase, Script)>,
    pub(super) name: Entity<InputState>,
    pub(super) command: Entity<InputState>,
    pub(super) timeout: Entity<InputState>,
    phase: Entity<SelectState<Vec<Choice>>>,
    task: Option<Task<()>>,
    closed: bool,
    error: Option<&'static str>,
}

pub(super) fn open(
    owner: Entity<Panel>,
    id: Option<String>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Editor> {
    let id = id.unwrap_or_else(|| sailry_protocol::RequestId::new().to_string());
    let previous = owner.read(cx).entry(&id);
    let script = previous
        .as_ref()
        .map(|(_, script)| script.clone())
        .unwrap_or(Script {
            enabled: false,
            ..Default::default()
        });
    let selected = previous.as_ref().map_or(0, |(phase, _)| {
        Phase::ALL
            .iter()
            .position(|current| current == phase)
            .unwrap()
    });
    let editor = cx.new(|cx| {
        crate::feedback::observe(window, cx, |editor: &Editor, _| {
            editor.error.into_iter().collect()
        });
        let name = cx.new(|cx| InputState::new(window, cx).default_value(script.name));
        let command = cx.new(|cx| InputState::new(window, cx).default_value(script.command));
        let timeout =
            cx.new(|cx| InputState::new(window, cx).default_value(script.timeout_ms.to_string()));
        let phase = cx.new(|cx| {
            SelectState::new(
                Phase::ALL.into_iter().map(Choice).collect::<Vec<_>>(),
                Some(IndexPath::new(selected)),
                window,
                cx,
            )
        });
        for input in [&name, &command, &timeout] {
            cx.subscribe_in(
                input,
                window,
                |editor: &mut Editor, _, event, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        editor.changed(window, cx);
                    }
                },
            )
            .detach();
        }
        cx.subscribe_in(
            &phase,
            window,
            |editor: &mut Editor, _, event, window, cx| {
                if matches!(event, SelectEvent::Confirm(_)) {
                    editor.changed(window, cx);
                }
            },
        )
        .detach();
        Editor {
            owner,
            id,
            previous,
            name,
            command,
            timeout,
            phase,
            task: None,
            closed: false,
            error: None,
        }
    });
    #[cfg(test)]
    {
        let owner = editor.read(cx).owner.clone();
        owner.update(cx, |panel, _| panel.editor = Some(editor.downgrade()));
    }
    window.open_dialog(cx, {
        let editor = editor.clone();
        move |dialog, window, _| {
            let close = editor.clone();
            let cancel = editor.clone();
            dialog
                .form_title(tr("hooks_configure"))
                .w((window.viewport_size().width - px(48.)).min(px(600.)))
                .overlay_closable(false)
                .on_cancel(move |_, window, cx| {
                    cancel.update(cx, |editor, cx| editor.finish(window, cx))
                })
                .child(editor.clone())
                .footer(
                    DialogFooter::new().child(
                        Button::new("hooks-editor-close")
                            .debug_selector(|| "hooks-editor-close".into())
                            .label(tr("close"))
                            .on_click(move |_, window, cx| {
                                if close.update(cx, |editor, cx| editor.finish(window, cx)) {
                                    window.close_dialog(cx);
                                }
                            }),
                    ),
                )
        }
    });
    editor
}

impl Editor {
    fn changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed {
            return;
        }
        self.error = None;
        self.task = Some(cx.spawn_in(window, async move |owner, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(350))
                .await;
            _ = owner.update_in(cx, |editor, window, cx| {
                editor.flush(false, window, cx);
            });
        }));
        cx.notify();
    }

    fn finish(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.owner.read(cx).active {
            self.error = Some("hooks_target_changed");
            self.closed = true;
            self.task = None;
            cx.notify();
            return true;
        }
        if self.flush(true, window, cx) {
            self.closed = true;
            self.task = None;
            true
        } else {
            false
        }
    }

    fn flush(&mut self, report: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let command = self.command.read(cx).value().to_string();
        if command.trim().is_empty() && self.previous.is_none() {
            return true;
        }
        let timeout = self.timeout.read(cx).value().parse::<u64>();
        let Some(timeout_ms) = timeout.ok().filter(|value| (1..=60_000).contains(value)) else {
            if report {
                self.error = Some("hooks_invalid");
                cx.notify();
            }
            return false;
        };
        if command.trim().is_empty() {
            if report {
                self.error = Some("hooks_invalid");
                cx.notify();
            }
            return false;
        }
        let phase = self
            .phase
            .read(cx)
            .selected_value()
            .copied()
            .unwrap_or(Phase::BeforeTool);
        let enabled = self.previous.as_ref().is_some_and(|(previous, script)| {
            *previous == phase
                && script.command == command
                && script.timeout_ms == timeout_ms
                && script.enabled
        });
        let script = Script {
            name: self.name.read(cx).value().to_string(),
            enabled,
            command,
            timeout_ms,
        };
        if self.previous.as_ref() == Some(&(phase, script.clone())) {
            return true;
        }
        let accepted = self.owner.update(cx, |panel, cx| {
            panel.edit(&self.id, &self.previous, phase, script.clone(), window, cx)
        });
        if accepted {
            self.previous = Some((phase, script));
        }
        accepted
    }
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_4()
            .child(
                Field::new().label(tr("hooks_name")).child(
                    div()
                        .debug_selector(|| "hooks-name".into())
                        .child(Input::new(&self.name).aria_label(tr("hooks_name"))),
                ),
            )
            .child(
                Field::new().label(tr("hooks_phase")).child(
                    div().debug_selector(|| "hooks-phase".into()).child(
                        Select::new(&self.phase)
                            .w_full()
                            .disabled(Phase::ALL.iter().any(|phase| phase.name() == self.id))
                            .accessibility_label(tr("hooks_phase")),
                    ),
                ),
            )
            .child(
                Field::new().label(tr("hooks_command")).child(
                    div()
                        .debug_selector(|| "hooks-command".into())
                        .child(Input::new(&self.command).aria_label(tr("hooks_command"))),
                ),
            )
            .child(
                Field::new().label(tr("hooks_timeout")).child(
                    div()
                        .debug_selector(|| "hooks-timeout".into())
                        .child(Input::new(&self.timeout).aria_label(tr("hooks_timeout"))),
                ),
            )
    }
}
