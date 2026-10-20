use super::*;
use crate::{theme::DialogStyle as _, tr};
use chrono::Timelike as _;
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        dialog::DialogFooter,
        form::Field,
        input::{Input, InputEvent, InputState},
        scroll::ScrollableElement,
        switch::Switch,
        tab::{Tab, TabBar},
        time_field::{TimeField, TimeFieldEvent, TimeFieldState},
    },
    prelude::FluentBuilder as _,
};
use sailry_protocol::{
    Secret,
    backup::{Destination, Kind, Local, Schedule, Update},
    plugin::settings::SecretUpdate,
};

fn destination(settings: &Settings) -> Destination {
    settings.destination.clone().unwrap_or(Destination {
        kind: Kind::Webdav,
        endpoint: String::new(),
        root: "sailry-backups".into(),
        bucket: String::new(),
        region: String::new(),
    })
}

pub(super) struct Editor {
    owner: WeakEntity<Panel>,
    tab: usize,
    settings: Settings,
    kind: Kind,
    time: Entity<TimeFieldState>,
    local_enabled: bool,
    online_enabled: bool,
    schedule_enabled: bool,
    pub(super) fields: Vec<(&'static str, Entity<InputState>)>,
    remove_credentials: bool,
    task: Option<Task<()>>,
    pub(super) conflict: bool,
    error: Option<&'static str>,
}

pub(super) fn create(
    owner: WeakEntity<Panel>,
    settings: Settings,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Editor> {
    let value = destination(&settings);
    cx.new(|cx| {
        crate::feedback::observe_with(
            window,
            cx,
            |editor: &Editor, _| editor.error.into_iter().collect(),
            |_, key, cx| {
                let owner = cx.weak_entity();
                notification::Notification::error(tr(key)).action(move |_, _, cx| {
                    let owner = owner.clone();
                    Button::new("backup-editor-reload")
                        .label(tr("backup_reload"))
                        .debug_selector(|| "backup-editor-reload".into())
                        .on_click(cx.listener(move |toast, _, window, cx| {
                            toast.dismiss(window, cx);
                            _ = owner.update(cx, |editor, cx| {
                                _ = editor.owner.update(cx, |panel, cx| {
                                    panel.rebase = true;
                                    panel.load(window, cx);
                                });
                            });
                        }))
                })
            },
        );
        let fields: Vec<_> = [
            ("backup_endpoint", value.endpoint),
            ("backup_prefix", value.root),
            ("backup_bucket", value.bucket),
            ("backup_region", value.region),
            ("backup_identity", String::new()),
            ("backup_secret", String::new()),
            ("backup_session_token", String::new()),
            ("backup_passphrase", String::new()),
            ("backup_confirm_passphrase", String::new()),
            (
                "backup_local_directory",
                settings
                    .local
                    .as_ref()
                    .map(|local| local.directory.clone())
                    .unwrap_or_default(),
            ),
            (
                "backup_timezone",
                settings
                    .schedule
                    .as_ref()
                    .map(|schedule| schedule.timezone.clone())
                    .unwrap_or_else(|| "UTC".into()),
            ),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (key, value))| {
            (
                key,
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(value)
                        .masked((4..9).contains(&index))
                        .placeholder(tr(
                            if (index == 7 && settings.passphrase_configured)
                                || ((4..7).contains(&index) && settings.credentials_configured)
                            {
                                "backup_keep_secret"
                            } else {
                                key
                            },
                        ))
                }),
            )
        })
        .collect();
        for (_, input) in &fields {
            cx.subscribe_in(
                input,
                window,
                move |editor: &mut Editor, _, event, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        editor.changed(window, cx);
                    }
                },
            )
            .detach();
        }
        let time = cx.new(|cx| {
            let mut state = TimeFieldState::new(window, cx);
            let (hour, minute) = settings
                .schedule
                .as_ref()
                .map(|schedule| (schedule.hour, schedule.minute))
                .unwrap_or((3, 0));
            state.set_time(
                chrono::NaiveTime::from_hms_opt(hour.into(), minute.into(), 0).unwrap_or_default(),
                window,
                cx,
            );
            state
        });
        cx.subscribe_in(
            &time,
            window,
            |editor: &mut Editor, _, _: &TimeFieldEvent, window, cx| editor.changed(window, cx),
        )
        .detach();
        Editor {
            owner,
            tab: usize::from(value.kind == Kind::S3),
            local_enabled: settings.local.as_ref().is_some_and(|local| local.enabled),
            online_enabled: settings.online_enabled,
            schedule_enabled: settings
                .schedule
                .as_ref()
                .is_some_and(|schedule| schedule.enabled),
            settings,
            kind: value.kind,
            time,
            fields,
            remove_credentials: false,
            task: None,
            conflict: false,
            error: None,
        }
    })
}

pub(super) fn open(owner: Entity<Panel>, window: &mut Window, cx: &mut App) {
    let Some(editor) = owner.read(cx).editor.clone() else {
        return;
    };
    window.open_dialog(cx, move |dialog, window, cx| {
        let close = editor.clone();
        let cancel = editor.clone();
        dialog
            .form_title(tr("backup_destination"), cx)
            .w((window.viewport_size().width - px(48.)).min(px(600.)))
            .overlay_closable(false)
            .on_cancel(move |_, window, cx| {
                cancel.update(cx, |editor, cx| editor.finish(window, cx))
            })
            .child(editor.clone())
            .footer(
                DialogFooter::new().child(
                    Button::new("backup-editor-close")
                        .debug_selector(|| "backup-editor-close".into())
                        .label(tr("close"))
                        .on_click(move |_, window, cx| {
                            if close.update(cx, |editor, cx| editor.finish(window, cx)) {
                                window.close_dialog(cx);
                            }
                        }),
                ),
            )
    });
}

impl Editor {
    fn values(&self, cx: &App) -> Vec<String> {
        self.fields
            .iter()
            .map(|(_, input)| input.read(cx).value().to_string())
            .collect()
    }
    fn update(&self, cx: &App) -> Result<Update, &'static str> {
        let values = self.values(cx);
        if !values[7].is_empty() || !values[8].is_empty() {
            sailry_protocol::backup::validate_passphrase(&values[7])
                .map_err(|_| "backup_passphrase_invalid")?;
            if values[7] != values[8] {
                return Err("backup_passphrase_mismatch");
            }
        }
        let kind = self.kind;
        let destination = Destination {
            kind,
            endpoint: values[0].trim().into(),
            root: values[1].trim().into(),
            bucket: values[2].trim().into(),
            region: values[3].trim().into(),
        };
        let destination = if destination.endpoint.is_empty() {
            if self.online_enabled && self.settings.destination.is_some() {
                return Err("backup_invalid");
            }
            None
        } else {
            destination.validate().map_err(|_| "backup_invalid")?;
            Some(destination)
        };
        let directory = values[9].trim();
        if self.local_enabled && directory.is_empty() {
            return Err("backup_invalid_target");
        }
        let time = self.time.read(cx).time();
        let schedule = Schedule {
            enabled: self.schedule_enabled,
            timezone: values[10].trim().to_owned(),
            hour: time.hour() as u8,
            minute: time.minute() as u8,
        };
        if self.schedule_enabled && schedule.timezone.is_empty() {
            return Err("backup_invalid");
        }
        let credential = |value: &str| {
            if !value.is_empty() {
                SecretUpdate::Replace(Secret::new(value.into()))
            } else if self.remove_credentials {
                SecretUpdate::Clear
            } else {
                SecretUpdate::Keep
            }
        };
        let mut settings = self.settings.clone();
        settings.destination = destination;
        settings.online_enabled = self.online_enabled;
        settings.local = (!directory.is_empty() || self.local_enabled).then(|| Local {
            directory: directory.to_owned(),
            enabled: self.local_enabled,
        });
        settings.schedule = (self.settings.schedule.is_some()
            || schedule.enabled
            || schedule.timezone != "UTC"
            || schedule.hour != 3
            || schedule.minute != 0)
            .then_some(schedule);
        Ok(Update {
            settings,
            identity: credential(&values[4]),
            secret: credential(&values[5]),
            session_token: if kind == Kind::S3 {
                credential(&values[6])
            } else {
                SecretUpdate::Clear
            },
            passphrase: if !values[7].is_empty() {
                SecretUpdate::Replace(Secret::new(values[7].clone()))
            } else {
                SecretUpdate::Keep
            },
        })
    }
    fn dirty(&self, cx: &App) -> bool {
        let values = self.values(cx);
        let destination = destination(&self.settings);
        let schedule = self.settings.schedule.clone().unwrap_or(Schedule {
            enabled: false,
            timezone: "UTC".into(),
            hour: 3,
            minute: 0,
        });
        let time = self.time.read(cx).time();
        self.remove_credentials
            || values[4..9].iter().any(|value| !value.is_empty())
            || self.kind != destination.kind
            || values[..4]
                .iter()
                .zip([
                    destination.endpoint,
                    destination.root,
                    destination.bucket,
                    destination.region,
                ])
                .any(|(value, saved)| value.trim() != saved)
            || self.local_enabled
                != self
                    .settings
                    .local
                    .as_ref()
                    .is_some_and(|local| local.enabled)
            || values[9].trim()
                != self
                    .settings
                    .local
                    .as_ref()
                    .map(|local| local.directory.as_str())
                    .unwrap_or_default()
            || self.online_enabled != self.settings.online_enabled
            || self.schedule_enabled != schedule.enabled
            || values[10].trim() != schedule.timezone
            || time.hour() != u32::from(schedule.hour)
            || time.minute() != u32::from(schedule.minute)
    }
    fn changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        self.task = Some(cx.spawn_in(window, async move |owner, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(350))
                .await;
            _ = owner.update_in(cx, |editor, window, cx| {
                editor.flush(false, window, cx);
            });
        }));
        cx.notify();
    }
    fn flush(&mut self, report: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(owner) = self.owner.upgrade() else {
            return false;
        };
        if !owner.read(cx).active {
            return false;
        }
        if self.conflict {
            if report {
                self.error = Some("backup_conflict");
                cx.notify();
            }
            return false;
        }
        if owner.read(cx).locked() {
            return false;
        }
        let update = match self.update(cx) {
            Ok(update) => update,
            Err(key) => {
                if report {
                    self.error = Some(key);
                    cx.notify();
                }
                return false;
            }
        };
        if !self.dirty(cx)
            || (update.settings == self.settings
                && !self.remove_credentials
                && self.fields[4..9]
                    .iter()
                    .all(|(_, input)| input.read(cx).value().is_empty()))
        {
            return true;
        }
        owner.update(cx, |panel, cx| {
            panel.submit(Command::SaveBackupSettings(update), window, cx)
        })
    }
    fn finish(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self
            .owner
            .upgrade()
            .is_none_or(|owner| !owner.read(cx).active)
        {
            self.error = Some("backup_target_changed");
            self.task = None;
            self.erase(window, cx);
            cx.notify();
            return true;
        }
        if self.flush(true, window, cx) {
            self.task = None;
            self.erase(window, cx);
            true
        } else {
            false
        }
    }
    pub(super) fn accept(
        &mut self,
        settings: Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings = settings;
        self.remove_credentials = false;
        self.conflict = false;
        self.error = None;
        self.erase(window, cx);
        if self.dirty(cx) {
            self.changed(window, cx);
        }
        cx.notify();
    }
    fn erase(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for (_, input) in &self.fields[4..9] {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
    }
    pub(super) fn observe(
        &mut self,
        settings: &Settings,
        rebase: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if settings.revision == self.settings.revision {
            return;
        }
        if !self.dirty(cx) {
            {
                let destination = destination(settings);
                for (index, value) in [
                    destination.endpoint.clone(),
                    destination.root.clone(),
                    destination.bucket.clone(),
                    destination.region.clone(),
                ]
                .into_iter()
                .enumerate()
                {
                    self.fields[index]
                        .1
                        .update(cx, |input, cx| input.set_value(value, window, cx));
                }
                self.kind = destination.kind;
            }
            self.local_enabled = settings.local.as_ref().is_some_and(|local| local.enabled);
            self.online_enabled = settings.online_enabled;
            self.schedule_enabled = settings
                .schedule
                .as_ref()
                .is_some_and(|schedule| schedule.enabled);
            let directory = settings
                .local
                .as_ref()
                .map(|local| local.directory.clone())
                .unwrap_or_default();
            let timezone = settings
                .schedule
                .as_ref()
                .map(|schedule| schedule.timezone.clone())
                .unwrap_or_else(|| "UTC".into());
            self.fields[9]
                .1
                .update(cx, |input, cx| input.set_value(directory, window, cx));
            self.fields[10]
                .1
                .update(cx, |input, cx| input.set_value(timezone, window, cx));
            let (hour, minute) = settings
                .schedule
                .as_ref()
                .map(|schedule| (schedule.hour, schedule.minute))
                .unwrap_or((3, 0));
            self.time.update(cx, |time, cx| {
                time.set_time(
                    chrono::NaiveTime::from_hms_opt(hour.into(), minute.into(), 0)
                        .unwrap_or_default(),
                    window,
                    cx,
                )
            });
            self.settings = settings.clone();
            self.conflict = false;
            self.error = None;
        } else if rebase {
            // Explicit reload retains the user's destination and temporary credential draft.
            self.settings.revision = settings.revision;
            self.settings.credentials_configured = settings.credentials_configured;
            self.settings.passphrase_configured = settings.passphrase_configured;
            self.conflict = false;
            self.error = None;
        } else {
            self.conflict = true;
            self.error = Some("backup_conflict");
        }
        cx.notify();
    }
}

impl Editor {
    pub(super) fn automatic(&mut self, locked: bool, cx: &mut Context<Self>) -> Vec<AnyElement> {
        use crate::settings::group::Row;
        let mut rows = vec![
            Row::new(
                "backup_schedule_enabled",
                Switch::new("backup-schedule-enabled")
                    .checked(self.schedule_enabled)
                    .accessibility_label(tr("backup_schedule_enabled"))
                    .disabled(locked)
                    .on_change(cx.listener(|editor, value, window, cx| {
                        editor.schedule_enabled = *value;
                        editor.changed(window, cx);
                    })),
            )
            .into_any_element(),
        ];
        if self.schedule_enabled {
            rows.push(
                Row::new(
                    "backup_schedule_time",
                    TimeField::new(&self.time).disabled(locked),
                )
                .into_any_element(),
            );
            rows.push(
                Row::new(
                    "backup_timezone",
                    Input::new(&self.fields[10].1).disabled(locked).w_48(),
                )
                .into_any_element(),
            );
        }
        rows
    }

    fn choose_directory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = self.owner.upgrade() else {
            return;
        };
        if panel.read(cx).locked() {
            return;
        }
        let generation = panel.read(cx).generation;
        let client = panel.read(cx).binding.client.clone();
        let initial = self.fields[9].1.read(cx).value().to_string();
        let owner = cx.weak_entity();
        let valid = owner.clone();
        let pending = owner.clone();
        let picker = crate::file_picker::open(
            crate::file_picker::Options {
                client: Some(client),
                preview_host: 0,
                initial: (!initial.is_empty()).then_some(initial),
                mode: crate::file_picker::Mode::Directory,
                title: "backup_local_directory",
                confirm: "directory_title",
            },
            move |selection, window, cx| {
                let crate::file_picker::Selection::Directory(path) = selection else {
                    return Err("directory_stale");
                };
                owner
                    .update(cx, |editor, cx| {
                        let Some(panel) = editor.owner.upgrade() else {
                            return Err("directory_stale");
                        };
                        if !panel.read(cx).active || panel.read(cx).generation != generation {
                            return Err("directory_stale");
                        }
                        editor.fields[9]
                            .1
                            .update(cx, |input, cx| input.set_value(path, window, cx));
                        editor.changed(window, cx);
                        Ok(crate::file_picker::Completion::Close)
                    })
                    .unwrap_or(Err("directory_stale"))
            },
            window,
            cx,
        );
        picker.update(cx, |picker, _| {
            picker.bind(
                move |cx| {
                    valid
                        .upgrade()
                        .and_then(|editor| editor.read(cx).owner.upgrade())
                        .is_some_and(|panel| {
                            panel.read(cx).active && panel.read(cx).generation == generation
                        })
                },
                move |cx| {
                    pending
                        .upgrade()
                        .and_then(|editor| editor.read(cx).owner.upgrade())
                        .is_none_or(|panel| panel.read(cx).locked())
                },
            )
        });
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locked = self
            .owner
            .upgrade()
            .is_none_or(|owner| owner.read(cx).locked());
        let s3 = self.tab == 1;
        let kind = if s3 { Kind::S3 } else { Kind::Webdav };
        let selected = self.kind;
        v_flex()
            .gap_4()
            .child(
                TabBar::new("backup-storage-tabs")
                    .segmented()
                    .selected_index(self.tab)
                    .on_click(cx.listener(|editor, index, _, cx| {
                        editor.tab = *index;
                        cx.notify();
                    }))
                    .child(Tab::new().label(tr("backup_webdav")))
                    .child(Tab::new().label(tr("backup_s3")))
                    .child(Tab::new().label(tr("backup_local"))),
            )
            .when(self.tab == 2, |view| {
                view.child(
                    Field::new().label(tr("backup_local_enabled")).child(
                        Switch::new("backup-local-enabled")
                            .checked(self.local_enabled)
                            .disabled(locked)
                            .accessibility_label(tr("backup_local_enabled"))
                            .on_change(cx.listener(|editor, value, window, cx| {
                                editor.local_enabled = *value;
                                editor.changed(window, cx);
                            })),
                    ),
                )
                .child(
                    Field::new().label(tr("backup_local_directory")).child(
                        h_flex()
                            .gap_2()
                            .child(Input::new(&self.fields[9].1).readonly(true).flex_1())
                            .child(
                                Button::new("backup-choose-directory")
                                    .label(tr("backup_choose_directory"))
                                    .disabled(locked)
                                    .on_click(cx.listener(|editor, _, window, cx| {
                                        editor.choose_directory(window, cx)
                                    })),
                            ),
                    ),
                )
            })
            .when(self.tab < 2, |view| {
                view.child(
                    Field::new().label(tr("backup_online_enabled")).child(
                        Switch::new("backup-online-enabled")
                            .checked(self.online_enabled && selected == kind)
                            .disabled(locked)
                            .accessibility_label(tr("backup_online_enabled"))
                            .on_change(cx.listener(move |editor, value, window, cx| {
                                editor.kind = kind;
                                editor.online_enabled = *value;
                                editor.changed(window, cx);
                            })),
                    ),
                )
                .children(
                    self.fields
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| *index < 7 && (s3 || !matches!(index, 2 | 3 | 6)))
                        .map(|(_, (key, input))| {
                            Field::new()
                                .label(tr(key))
                                .child(Input::new(input).disabled(locked || selected != kind))
                        }),
                )
            })
            .children(self.fields[7..9].iter().map(|(key, input)| {
                Field::new()
                    .label(tr(key))
                    .child(Input::new(input).disabled(locked))
            }))
            .when(
                self.settings.credentials_configured && self.tab < 2,
                |view| {
                    view.child(
                        Button::new("backup-clear-credentials")
                            .ghost()
                            .label(tr("backup_clear_credentials"))
                            .disabled(locked)
                            .on_click(cx.listener(|editor, _, window, cx| {
                                editor.remove_credentials = true;
                                editor.changed(window, cx);
                            })),
                    )
                },
            )
            .max_h((window.viewport_size().height - px(260.)).max(px(180.)))
            .overflow_y_scrollbar()
    }
}
