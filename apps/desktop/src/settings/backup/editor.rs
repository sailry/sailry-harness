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
    RequestId, Secret,
    backup::{Destination, Kind, Local, Schedule, Update},
    plugin::settings::SecretUpdate,
};

mod draft;
use draft::Draft;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Schedule,
    Storage,
}

fn destination(settings: &Settings) -> Destination {
    settings.destination.clone().unwrap_or(Destination {
        kind: Kind::Webdav,
        endpoint: String::new(),
        root: "sailry-backups".into(),
        bucket: String::new(),
        region: String::new(),
    })
}

fn storage_tab(settings: &Settings) -> usize {
    if !settings.online_enabled && settings.local.as_ref().is_some_and(|local| local.enabled) {
        2
    } else {
        usize::from(destination(settings).kind == Kind::S3)
    }
}

fn storage(
    settings: &mut Settings,
    tab: usize,
    mut destination: Destination,
    directory: &str,
) -> Result<(), &'static str> {
    if tab == 2 {
        if directory.is_empty() {
            return Err("backup_invalid_target");
        }
    } else {
        destination.kind = if tab == 1 { Kind::S3 } else { Kind::Webdav };
        settings.destination = if destination.endpoint.is_empty() {
            if settings.destination.is_some() || storage_tab(settings) != tab {
                return Err("backup_invalid");
            }
            None
        } else {
            destination.validate().map_err(|_| "backup_invalid")?;
            Some(destination)
        };
    }
    settings.online_enabled = tab < 2;
    settings.local = (!directory.is_empty()).then(|| Local {
        directory: directory.to_owned(),
        enabled: tab == 2,
    });
    Ok(())
}

pub(super) struct Editor {
    owner: WeakEntity<Panel>,
    mode: Mode,
    tab: usize,
    settings: Settings,
    time: Entity<TimeFieldState>,
    schedule_enabled: bool,
    pub(super) fields: Vec<(&'static str, Entity<InputState>)>,
    remove_credentials: bool,
    task: Option<Task<()>>,
    pub(super) conflict: bool,
    error: Option<&'static str>,
    submitted: Option<RequestId>,
    closed: bool,
    dialog: crate::ui::overlays::Owned,
}

pub(super) fn create(
    owner: WeakEntity<Panel>,
    settings: Settings,
    mode: Mode,
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
            mode,
            tab: storage_tab(&settings),
            schedule_enabled: settings
                .schedule
                .as_ref()
                .is_some_and(|schedule| schedule.enabled),
            settings,
            time,
            fields,
            remove_credentials: false,
            task: None,
            conflict: false,
            error: None,
            submitted: None,
            closed: false,
            dialog: Default::default(),
        }
    })
}

pub(super) fn open(owner: Entity<Panel>, window: &mut Window, cx: &mut App) {
    if owner.read(cx).locked() {
        return;
    }
    let Some(settings) = owner.read(cx).settings.clone() else {
        return;
    };
    let editor = create(owner.downgrade(), settings, Mode::Storage, window, cx);
    editor.update(cx, |_, cx| {
        cx.observe_in(&owner, window, |editor, owner, window, cx| {
            if !owner.read(cx).active {
                editor.closed = true;
                editor.dialog.close(window, cx);
            }
            cx.notify();
        })
        .detach();
    });
    owner.update(cx, |panel, cx| {
        panel.storage = Some(editor.clone());
        cx.notify();
    });
    let active = editor.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        let close = editor.clone();
        let cancel = editor.clone();
        let confirm = editor.clone();
        let save = editor.clone();
        let state = editor.read(cx);
        let retry = state.retry(cx);
        let pending = state
            .owner
            .upgrade()
            .is_some_and(|owner| owner.read(cx).pending);
        dialog
            .form_title(tr("backup_destination"), cx)
            .w((window.viewport_size().width - px(48.)).min(px(600.)))
            .overlay_closable(false)
            .on_ok(move |_, window, cx| {
                confirm.update(cx, |editor, cx| editor.save(window, cx));
                false
            })
            .on_close(move |_, window, cx| {
                dismiss(&close, window, cx);
            })
            .child(editor.clone())
            .footer(
                DialogFooter::new()
                    .w_full()
                    .gap_2()
                    .child(
                        Button::new("backup-editor-cancel")
                            .debug_selector(|| "backup-editor-cancel".into())
                            .label(tr("settings_cancel"))
                            .on_click(move |_, window, cx| {
                                dismiss(&cancel, window, cx);
                                window.close_dialog(cx);
                            }),
                    )
                    .child(
                        Button::new("backup-editor-save")
                            .debug_selector(|| "backup-editor-save".into())
                            .primary()
                            .label(tr(if retry {
                                "plugins_retry"
                            } else {
                                "settings_save"
                            }))
                            .loading(pending && retry)
                            .disabled(
                                state.closed
                                    || state.conflict
                                    || pending
                                    || (state.locked(cx) && !retry),
                            )
                            .on_click(move |_, window, cx| {
                                save.update(cx, |editor, cx| editor.save(window, cx));
                            }),
                    ),
            )
    });
    active.update(cx, |editor, cx| {
        crate::ui::overlays::Owned::bind(editor, |editor| &mut editor.dialog, window, cx)
    });
}

fn dismiss(editor: &Entity<Editor>, window: &mut Window, cx: &mut App) {
    let owner = editor.update(cx, |editor, cx| {
        editor.task = None;
        editor.error = None;
        editor.closed = true;
        editor.dialog.dismissed();
        editor.erase(window, cx);
        editor.owner.upgrade()
    });
    if let Some(owner) = owner {
        owner.update(cx, |panel, cx| {
            if panel.storage.as_ref() != Some(editor) {
                return;
            }
            panel.storage = None;
            cx.notify();
        });
    }
}

impl Editor {
    fn values(&self, cx: &App) -> Vec<String> {
        self.fields
            .iter()
            .map(|(_, input)| input.read(cx).value().to_string())
            .collect()
    }
    fn draft(&self, cx: &App) -> Draft {
        let values = self.values(cx);
        let time = self.time.read(cx).time();
        Draft {
            tab: self.tab,
            destination: Destination {
                kind: if self.tab == 1 {
                    Kind::S3
                } else {
                    Kind::Webdav
                },
                endpoint: values[0].trim().into(),
                root: values[1].trim().into(),
                bucket: values[2].trim().into(),
                region: values[3].trim().into(),
            },
            directory: values[9].trim().into(),
            identity: values[4].clone(),
            secret: values[5].clone(),
            session_token: values[6].clone(),
            passphrase: values[7].clone(),
            confirmation: values[8].clone(),
            schedule: Schedule {
                enabled: self.schedule_enabled,
                timezone: values[10].trim().into(),
                hour: time.hour() as u8,
                minute: time.minute() as u8,
            },
            remove_credentials: self.remove_credentials,
        }
    }
    fn update(&self, cx: &App) -> Result<Update, &'static str> {
        self.draft(cx).update(&self.settings, self.mode)
    }
    fn dirty(&self, cx: &App) -> bool {
        self.draft(cx).dirty(&self.settings, self.mode)
    }
    fn locked(&self, cx: &App) -> bool {
        self.closed
            || self
                .owner
                .upgrade()
                .is_none_or(|owner| owner.read(cx).locked())
    }
    fn retry(&self, cx: &App) -> bool {
        self.owner.upgrade().is_some_and(|owner| {
            owner
                .read(cx)
                .request
                .as_ref()
                .is_some_and(|request| Some(request.id) == self.submitted)
        })
    }
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        if self.closed
            || self.conflict
            || !owner.read(cx).active
            || owner.read(cx).storage.as_ref() != Some(&cx.entity())
        {
            return;
        }
        if self.retry(cx) {
            owner.update(cx, |panel, cx| panel.retry(window, cx));
            return;
        }
        if self.locked(cx) {
            return;
        }
        let update = match self.update(cx) {
            Ok(update) => update,
            Err(key) => {
                self.error = Some(key);
                cx.notify();
                return;
            }
        };
        if !self.dirty(cx) {
            self.closed = true;
            self.dialog.close(window, cx);
            owner.update(cx, |panel, cx| {
                panel.storage = None;
                cx.notify();
            });
            return;
        }
        self.submitted = owner.update(cx, |panel, cx| {
            panel
                .submit(Command::SaveBackupSettings(update), window, cx)
                .then(|| panel.request.as_ref().expect("submitted backup request").id)
        });
        self.error = None;
        cx.notify();
    }
    pub(super) fn submitted(&self, request: RequestId) -> bool {
        self.submitted == Some(request)
    }
    pub(super) fn saved(
        &mut self,
        settings: Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.accept(settings, window, cx);
        self.closed = true;
        self.dialog.close(window, cx);
    }
    fn changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        if self.mode == Mode::Storage || self.closed {
            cx.notify();
            return;
        }
        self.task = Some(cx.spawn_in(window, async move |owner, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(350))
                .await;
            _ = owner.update_in(cx, |editor, window, cx| {
                editor.flush(window, cx);
            });
        }));
        cx.notify();
    }
    fn flush(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.mode != Mode::Schedule || self.closed {
            return false;
        }
        let Some(owner) = self.owner.upgrade() else {
            return false;
        };
        if !owner.read(cx).active
            || owner.read(cx).editor.as_ref() != Some(&cx.entity())
            || self.conflict
        {
            return false;
        }
        if owner.read(cx).locked() {
            return false;
        }
        let update = match self.update(cx) {
            Ok(update) => update,
            Err(_) => return false,
        };
        if !self.dirty(cx) {
            return true;
        }
        owner.update(cx, |panel, cx| {
            panel.submit(Command::SaveBackupSettings(update), window, cx)
        })
    }
    pub(super) fn accept(
        &mut self,
        settings: Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.dirty(cx) {
            // A dismissed editor may have been replaced while this save was
            // admitted. Rebase its clean fields instead of resubmitting them.
            self.observe(&settings, false, window, cx);
        }
        self.settings = settings;
        self.remove_credentials = false;
        self.conflict = false;
        self.error = None;
        self.erase(window, cx);
        if self.mode == Mode::Schedule && self.dirty(cx) {
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
            }
            self.tab = storage_tab(settings);
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
            self.settings = settings.clone();
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
        v_flex()
            .gap_4()
            .child(
                TabBar::new("backup-storage-tabs")
                    .segmented()
                    .menu(false)
                    .w_full()
                    .selected_index(self.tab)
                    .on_click(cx.listener(|editor, index, window, cx| {
                        editor.tab = *index;
                        editor.changed(window, cx);
                    }))
                    .children(["backup_webdav", "backup_s3", "backup_local"].map(|key| {
                        Tab::new()
                            .flex_1()
                            .min_w_0()
                            .label(tr(key))
                            .disabled(locked)
                    })),
            )
            .when(self.tab == 2, |view| {
                view.child(
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
                view.children(
                    self.fields
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| *index < 7 && (s3 || !matches!(index, 2 | 3 | 6)))
                        .map(|(_, (key, input))| {
                            Field::new()
                                .label(tr(key))
                                .child(Input::new(input).disabled(locked))
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

#[cfg(test)]
#[path = "editor_tests.rs"]
mod tests;
