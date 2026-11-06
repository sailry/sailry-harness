use super::shell::Shell;
use crate::feedback::Recovery as _;
use crate::{
    settings::{
        draft,
        entry::Entry,
        group::{Group, Row},
        providers::Binding,
    },
    tr,
};
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        select::{Select, SelectEvent, SelectState},
        *,
    },
    *,
};
use sailry_protocol::{
    Command, ErrorCode, Output,
    terminal::{self, Settings},
};

pub(in crate::settings) struct Host {
    pub environment: Vec<(String, String)>,
    pub shell: Entity<SelectState<Vec<Shell>>>,
    shells: Vec<terminal::Shell>,
    binding: Option<Binding>,
    pub(super) saved: Option<Settings>,
    observed: Option<u64>,
    pending: bool,
    pub(super) error: Option<&'static str>,
}

impl Host {
    pub fn new(binding: Option<Binding>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        crate::feedback::observe_with(
            window,
            cx,
            |view: &Self, _| view.error.into_iter().collect(),
            |view, key, cx| {
                let owner = cx.weak_entity();
                let label = tr(if view.dirty(cx) {
                    "terminal_settings_discard_reload"
                } else {
                    "terminal_settings_reload"
                });
                gpui_kit::component::notification::Notification::error(tr(key)).scoped_action(
                    move |_, _, cx| {
                        let owner = owner.clone();
                        Button::new("terminal-settings-reload")
                            .label(label.clone())
                            .on_click(cx.listener(move |toast, _, window, cx| {
                                toast.dismiss(window, cx);
                                _ = owner.update(cx, |view, cx| view.load(window, cx));
                            }))
                    },
                )
            },
        );
        let shell = cx.new(|cx| {
            SelectState::new(
                Shell::items(&[], &terminal::Shell::default()),
                Some(IndexPath::new(0)),
                window,
                cx,
            )
        });
        cx.subscribe_in(&shell, window, |this, _, event, window, cx| {
            if matches!(event, SelectEvent::Confirm(_))
                && this.dirty(cx)
                && this.error != Some("terminal_settings_unknown")
            {
                this.save(window, cx);
            }
        })
        .detach();
        Self {
            environment: Vec::new(),
            shell,
            shells: Vec::new(),
            binding,
            saved: None,
            observed: None,
            pending: false,
            error: None,
        }
    }

    fn draft(&self, cx: &App) -> Settings {
        snapshot(
            self.saved.as_ref(),
            self.shell.read(cx).selected_value(),
            &self.environment,
        )
    }

    fn dirty(&self, cx: &App) -> bool {
        self.saved
            .as_ref()
            .is_some_and(|saved| *saved != self.draft(cx))
    }

    pub fn accept(&mut self, revision: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.observed == Some(revision) {
            return;
        }
        self.observed = Some(revision);
        if self
            .saved
            .as_ref()
            .is_some_and(|saved| saved.revision == revision)
        {
            return;
        }
        if self.dirty(cx) {
            self.error = Some("terminal_settings_conflict");
        } else {
            self.load(window, cx);
        }
    }

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.submit(Command::ReadTerminalSettings, window, cx);
    }

    fn select_shell(&mut self, saved: &Settings, window: &mut Window, cx: &mut Context<Self>) {
        let value = terminal::Shell {
            path: saved.shell.clone(),
            arguments: saved.shell_arguments.clone(),
        };
        self.shell.update(cx, |select, cx| {
            select.set_items(Shell::items(&self.shells, &value), window, cx);
            select.set_selected_value(&value, window, cx);
        });
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let data = self.draft(cx);
        if data.validate().is_err() {
            self.error = Some("terminal_settings_invalid");
            cx.notify();
            return;
        }
        if self.binding.is_none() {
            self.saved = Some(data);
            self.error = None;
            cx.notify();
            return;
        }
        self.submit(Command::SaveTerminalSettings(data), window, cx);
    }

    fn submit(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        let Some(binding) = self.binding.clone() else {
            return;
        };
        self.pending = true;
        self.error = None;
        let writing = command.durable();
        let request = binding.client.prepare(command);
        let job = binding.runtime.spawn(async move {
            let result = binding.client.execute(request).await;
            let shells = if !writing {
                Some(
                    binding
                        .client
                        .execute(binding.client.prepare(Command::ListShells))
                        .await,
                )
            } else {
                None
            };
            (result, shells)
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |this, window, cx| {
                this.pending = false;
                match result {
                    Ok((Ok(Output::TerminalSettings(saved)), shells)) => {
                        match shells {
                            Some(Ok(Output::Shells(items))) => this.shells = items,
                            Some(_) => this.error = Some("terminal_shells_failed"),
                            None => {}
                        }
                        this.environment = saved
                            .environment
                            .iter()
                            .map(|(key, value)| (key.clone(), value.clone()))
                            .collect();
                        this.select_shell(&saved, window, cx);
                        this.saved = Some(saved);
                    }
                    Ok((Err(error), _)) => {
                        this.error = Some(match error.code {
                            ErrorCode::RevisionConflict => "terminal_settings_conflict",
                            ErrorCode::InvalidRequest => "terminal_settings_invalid",
                            ErrorCode::OutcomeUnknown | ErrorCode::Unavailable if writing => {
                                "terminal_settings_unknown"
                            }
                            _ => "terminal_settings_failed",
                        })
                    }
                    _ => {
                        this.error = Some(if writing {
                            "terminal_settings_unknown"
                        } else {
                            "terminal_settings_failed"
                        })
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let disabled = self.pending || self.binding.is_some() && self.saved.is_none();
        let mut environment = Group::new("terminal_environment");
        for (index, (key, value)) in self.environment.iter().enumerate() {
            let owner = cx.entity();
            environment = environment.child(
                Entry::new(
                    format!("environment-{index}"),
                    v_flex()
                        .gap_1()
                        .child(div().truncate().child(key.clone()))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .truncate()
                                .child(value.clone()),
                        ),
                )
                .action(
                    Button::new(("environment-edit", index))
                        .icon(IconName::Settings2)
                        .tooltip(tr("settings_edit"))
                        .accessibility_label(tr("settings_edit"))
                        .disabled(disabled)
                        .on_click(move |_, window, cx| {
                            edit(owner.clone(), Some(index), window, cx)
                        }),
                )
                .action(
                    Button::new(("environment-delete", index))
                        .icon(IconName::CircleX)
                        .tooltip(tr("settings_delete"))
                        .accessibility_label(tr("settings_delete"))
                        .disabled(disabled)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.environment.remove(index);
                            this.save(window, cx);
                        })),
                ),
            );
        }
        let owner = cx.entity();
        environment = environment.action(
            Button::new("environment-add")
                .primary()
                .debug_selector(|| "environment-add".into())
                .label(tr("settings_add"))
                .disabled(disabled)
                .on_click(move |_, window, cx| edit(owner.clone(), None, window, cx)),
        );
        v_flex().gap_6().child(environment).child(
            Group::new("terminal_shell_group").child(
                Row::new(
                    "terminal_shell",
                    div()
                        .debug_selector(|| "terminal-shell".into())
                        .w_full()
                        .child(
                            Select::new(&self.shell)
                                .w_full()
                                .disabled(disabled)
                                .accessibility_label(tr("terminal_shell")),
                        ),
                )
                .wide(),
            ),
        )
    }
}

fn snapshot(
    saved: Option<&Settings>,
    selected: Option<&terminal::Shell>,
    environment: &[(String, String)],
) -> Settings {
    let mut data = saved.cloned().unwrap_or_default();
    data.environment = environment.iter().cloned().collect();
    if let Some(shell) = selected {
        data.shell = shell.path.clone();
        data.shell_arguments = shell.arguments.clone();
    }
    data
}

fn edit(owner: Entity<Host>, index: Option<usize>, window: &mut Window, cx: &mut App) {
    let value = index
        .map(|index| owner.read(cx).environment[index].clone())
        .unwrap_or_default();
    draft::open(
        "terminal_environment_add",
        vec![("terminal_variable", value.0), ("terminal_value", value.1)],
        move |values, window, cx| {
            owner.update(cx, |this, cx| {
                let key = values[0].trim();
                sailry_protocol::terminal::validate_key(key)
                    .map_err(|_| "terminal_variable_invalid")?;
                if this
                    .environment
                    .iter()
                    .enumerate()
                    .any(|(i, (existing, _))| Some(i) != index && existing == key)
                {
                    return Err("terminal_variable_duplicate");
                }
                let value = (key.to_owned(), values[1].clone());
                if let Some(index) = index {
                    this.environment[index] = value;
                } else {
                    this.environment.push(value);
                }
                this.save(window, cx);
                Ok(())
            })
        },
        window,
        cx,
    );
}

#[cfg(test)]
#[path = "host_tests.rs"]
mod tests;
