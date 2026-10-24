use super::super::entry::Entry;
use super::*;
use crate::theme::DialogStyle as _;
use gpui_kit::component::{
    accordion::Accordion,
    dialog::{DialogClose, DialogFooter},
    group_box::{GroupBox, GroupBoxVariants},
    spinner::Spinner,
};
use gpui_kit::prelude::FluentBuilder as _;
use sailry_link::CancellationToken;
use std::path::PathBuf;
use theme::package::Loaded;

pub(super) fn choose(window: &mut Window, cx: &mut App) {
    let selected = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: true,
        multiple: false,
        prompt: Some(tr("appearance_import")),
    });
    window
        .spawn(cx, async move |cx| match selected.await {
            Ok(Ok(Some(paths))) => {
                if let Some(path) = paths.into_iter().next() {
                    let _ = cx.update(|window, cx| load(path, window, cx));
                }
            }
            Ok(Ok(None)) => {}
            _ => {
                let _ = cx.update(|_, cx| fail("appearance_picker_failed", cx));
            }
        })
        .detach();
}

fn fail(key: &'static str, cx: &mut App) {
    cx.update_global::<theme::Catalog, _>(|catalog, _| catalog.error = Some(key));
}

pub(super) fn load(path: PathBuf, window: &mut Window, cx: &mut App) {
    let work = cx
        .background_executor()
        .spawn(async move { Loaded::read(&path, true) });
    window
        .spawn(cx, async move |cx| {
            let result = work.await;
            let _ = cx.update(|window, cx| match result {
                Ok(loaded) => preview(loaded, window, cx),
                Err(error) => {
                    eprintln!("could not import theme: {error}");
                    fail("appearance_import_invalid", cx);
                }
            });
        })
        .detach();
}

pub(super) fn preview(loaded: Loaded, window: &mut Window, cx: &mut App) {
    open(
        Some(theme::Package::preview(&loaded)),
        Some(loaded),
        None,
        None,
        CancellationToken::new(),
        window,
        cx,
    );
}

pub(super) fn details(
    package: theme::Package,
    updates: Entity<updates::State>,
    window: &mut Window,
    cx: &mut App,
) {
    open(
        Some(package),
        None,
        Some(updates),
        None,
        CancellationToken::new(),
        window,
        cx,
    );
}

fn open(
    package: Option<theme::Package>,
    loaded: Option<Loaded>,
    updates: Option<Entity<updates::State>>,
    source: Option<theme::market::Selection>,
    stop: CancellationToken,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Import> {
    let importing = loaded.is_some();
    let view = cx.new(|_| Import {
        package,
        loaded,
        busy: false,
        error: None,
        license_open: false,
        source,
        updates: updates.clone(),
        stop,
        loading: false,
        read: None,
        dialog: Default::default(),
        closed: false,
    });
    view.update(cx, |view, cx| {
        crate::feedback::observe(window, cx, |view, _| view.error.into_iter().collect());
        if let Some(updates) = &updates {
            cx.observe(updates, |_, _, cx| cx.notify()).detach();
        }
        view.load(window, cx);
    });
    let result = view.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        let busy = view.read(cx).busy;
        let id = view.read(cx).id().to_owned();
        let installed = cx
            .global::<theme::Catalog>()
            .packages
            .iter()
            .any(|package| package.id == id);
        let replacing = importing
            && cx
                .global::<theme::Catalog>()
                .packages
                .iter()
                .any(|package| package.id == id);
        let save = view.clone();
        let install = view.clone();
        let close = view.clone();
        let update = updates
            .as_ref()
            .is_some_and(|updates| updates.read(cx).available(&id));
        let pending = updates
            .as_ref()
            .is_some_and(|updates| updates.read(cx).pending(&id));
        dialog
            .form_title(
                div()
                    .pr_6()
                    .min_w_0()
                    .truncate()
                    .debug_selector(|| "theme-preview-title".into())
                    .child(view.read(cx).name()),
                cx,
            )
            .w((window.viewport_size().width - px(48.)).min(px(640.)))
            .close_button(!busy)
            .keyboard(!busy)
            .overlay_closable(!busy)
            .on_close(move |_, _, cx| {
                close.update(cx, |view, cx| {
                    view.dismiss(cx);
                    view.dialog.dismissed();
                });
            })
            .child(view.clone())
            .footer(
                DialogFooter::new()
                    .w_full()
                    .child(
                        h_flex()
                            .w_auto()
                            .flex_shrink_0()
                            .child(DialogClose::new().trigger(|button| {
                                button
                                    .label(tr(if importing {
                                        "settings_cancel"
                                    } else {
                                        "close"
                                    }))
                                    .disabled(busy)
                                    .debug_selector(|| "theme-import-cancel".into())
                            })),
                    )
                    .when(importing, |footer| {
                        footer.child(
                            Button::new("theme-import-confirm")
                                .primary()
                                .label(tr(if replacing {
                                    "appearance_replace"
                                } else {
                                    "appearance_import"
                                }))
                                .debug_selector(|| "theme-import-confirm".into())
                                .disabled(busy)
                                .loading(busy)
                                .on_click(move |_, window, cx| {
                                    save.update(cx, |view, cx| view.save(window, cx))
                                }),
                        )
                    })
                    .when(!importing && installed, |footer| {
                        let apply = id.clone();
                        footer.child(
                            Button::new("theme-details-apply")
                                .label(tr("appearance_select"))
                                .on_click(move |_, window, cx| {
                                    super::menu::apply(&apply, window, cx)
                                }),
                        )
                    })
                    .when(
                        !importing && !installed && view.read(cx).source.is_some(),
                        |footer| {
                            footer.child(
                                Button::new("theme-details-install")
                                    .primary()
                                    .label(tr("market_install"))
                                    .disabled(pending || view.read(cx).package.is_none())
                                    .loading(pending)
                                    .on_click(move |_, window, cx| {
                                        install.update(cx, |view, cx| view.install(window, cx))
                                    }),
                            )
                        },
                    )
                    .when(installed && (update || pending), |footer| {
                        let updates = updates.clone().expect("theme updates are bound");
                        footer.child(
                            Button::new("theme-details-update")
                                .primary()
                                .label(tr("market_update"))
                                .disabled(pending)
                                .loading(pending)
                                .on_click(move |_, window, cx| {
                                    updates.update(cx, |state, cx| state.update(&id, window, cx))
                                }),
                        )
                    }),
            )
    });
    result.update(cx, |view, cx| {
        crate::ui::overlays::Owned::bind(view, |view| &mut view.dialog, window, cx)
    });
    result
}

pub(in crate::settings) struct Closed;

pub(in crate::settings) struct Import {
    package: Option<theme::Package>,
    loaded: Option<Loaded>,
    busy: bool,
    error: Option<&'static str>,
    license_open: bool,
    source: Option<theme::market::Selection>,
    updates: Option<Entity<updates::State>>,
    stop: CancellationToken,
    loading: bool,
    read: Option<Task<()>>,
    dialog: crate::ui::overlays::Owned,
    closed: bool,
}

impl EventEmitter<Closed> for Import {}

impl Drop for Import {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Import {
    fn dismiss(&mut self, cx: &mut Context<Self>) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.stop.cancel();
        self.read = None;
        cx.emit(Closed);
    }

    fn id(&self) -> &str {
        self.package.as_ref().map_or_else(
            || {
                self.source
                    .as_ref()
                    .expect("theme details have a source")
                    .entry
                    .id
                    .as_str()
            },
            |package| package.id.as_str(),
        )
    }

    fn name(&self) -> SharedString {
        self.package.as_ref().map_or_else(
            || {
                self.source
                    .as_ref()
                    .expect("theme details have a source")
                    .entry
                    .name
                    .clone()
                    .into()
            },
            |package| package.name.clone(),
        )
    }

    fn load(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.package.is_some() || self.read.is_some() || self.stop.is_cancelled() {
            return;
        }
        let Some(source) = self.source.clone() else {
            return;
        };
        let Some(service) = self
            .updates
            .as_ref()
            .and_then(|updates| updates.read(cx).service.clone())
        else {
            self.error = Some("appearance_load_failed");
            cx.notify();
            return;
        };
        self.loading = true;
        self.error = None;
        let stop = self.stop.clone();
        let cancelled = stop.clone();
        let runtime = cx.global::<crate::backend::Services>().runtime.clone();
        let job = runtime.spawn(async move { service.download(source, &cancelled).await });
        self.read = Some(cx.spawn(async move |view, cx| {
            let result = job.await;
            _ = view.update(cx, |view, cx| {
                if stop.is_cancelled() {
                    return;
                }
                view.loading = false;
                view.read = None;
                match result {
                    Ok(Ok(prepared)) => {
                        view.package = Some(theme::Package::preview(&prepared.loaded))
                    }
                    _ => view.error = Some("appearance_load_failed"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn install(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.stop.is_cancelled() || self.package.is_none() {
            return;
        }
        let (Some(source), Some(updates)) = (&self.source, &self.updates) else {
            return;
        };
        if cx
            .global::<theme::Catalog>()
            .packages
            .iter()
            .any(|package| package.id == source.entry.id)
            || updates.read(cx).pending(&source.entry.id)
        {
            return;
        }
        updates.update(cx, |state, cx| state.install(source.clone(), window, cx));
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let catalog = cx.global::<theme::Catalog>();
        if let Err(error) = catalog.check(self.loaded.as_ref().expect("theme import snapshot")) {
            eprintln!("could not import theme: {error}");
            self.error = Some("appearance_import_conflict");
            cx.notify();
            return;
        }
        let root = catalog.root.clone();
        let loaded = self.loaded.take().expect("theme import snapshot");
        self.busy = true;
        self.error = None;
        cx.notify();
        let work = cx.background_executor().spawn(async move {
            let result = root.as_ref().map_or(Ok(()), |root| loaded.install(root));
            (loaded, result)
        });
        cx.spawn_in(window, async move |view, cx| {
            let (loaded, result) = work.await;
            let _ = view.update_in(cx, |view, window, cx| {
                view.busy = false;
                match result {
                    Ok(()) => {
                        let result = cx.update_global::<theme::Catalog, _>(|catalog, _| {
                            let active =
                                catalog.packages[catalog.selected].id == loaded.manifest.id;
                            catalog.add(loaded)?;
                            catalog.error = None;
                            Ok::<_, String>(
                                active.then(|| catalog.packages[catalog.selected].clone()),
                            )
                        });
                        match result {
                            Ok(package) => {
                                if let Some(package) = package {
                                    theme::apply_colors(package, Some(window), cx);
                                }
                                cx.refresh_windows();
                                crate::feedback::toast(
                                    window,
                                    tr("appearance_import_done"),
                                    notification::Notification::success(tr(
                                        "appearance_import_done",
                                    )),
                                    cx,
                                );
                            }
                            Err(error) => {
                                eprintln!("could not register imported theme: {error}");
                                fail("appearance_load_failed", cx);
                            }
                        }
                        window.close_dialog(cx);
                    }
                    Err(error) => {
                        eprintln!("could not save imported theme: {error}");
                        view.loaded = Some(loaded);
                        view.error = Some(if error.recovery.is_some() {
                            "appearance_save_uncertain"
                        } else {
                            "appearance_save_failed"
                        });
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }
}

impl Render for Import {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(package) = self.package.as_ref() else {
            return div()
                .w_full()
                .h_24()
                .flex()
                .items_center()
                .justify_center()
                .when(self.loading, |body| body.child(Spinner::new()))
                .when(!self.loading, |body| {
                    body.child(
                        crate::feedback::retry("theme-details-retry")
                            .on_click(cx.listener(|view, _, window, cx| view.load(window, cx))),
                    )
                })
                .into_any_element();
        };
        let Some(manifest) = package.manifest.as_ref() else {
            return theme::preview(package, cx.theme().is_dark(), cx).into_any_element();
        };
        let license = &manifest.license;
        let width = (window.viewport_size().width - px(48.)).min(px(640.));
        let value_width = (width - px(88.)) * 0.55;
        let value = |text: String| {
            div()
                .w(value_width)
                .min_w_0()
                .text_right()
                .whitespace_normal()
                .child(text)
        };
        v_flex()
            .id("theme-preview-details")
            .debug_selector(|| "theme-preview-details".into())
            .gap_4()
            // Kit's Dialog owns the single bounded scroll area and fixed footer.
            .flex_none()
            .p_1()
            .child(
                div()
                    .grid()
                    .grid_cols(if width < px(520.) { 1 } else { 2 })
                    .gap_3()
                    .children([false, true].map(|dark| {
                        let selector = if dark {
                            "theme-preview-dark"
                        } else {
                            "theme-preview-light"
                        };
                        GroupBox::new()
                            .id(selector)
                            .outline()
                            .min_w_0()
                            .content_style(
                                StyleRefinement::default()
                                    .p_0()
                                    .gap_0()
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .rounded(cx.theme().radius_lg)
                                    .overflow_hidden(),
                            )
                            .child(
                                div()
                                    .debug_selector(move || selector.into())
                                    .child(theme::preview(package, dark, cx)),
                            )
                            .child(
                                div()
                                    .px_3()
                                    .py_2()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(tr(if dark {
                                        "appearance_dark"
                                    } else {
                                        "appearance_light"
                                    })),
                            )
                    })),
            )
            .when_some(manifest.description.clone(), |body, description| {
                body.child(
                    div()
                        .debug_selector(|| "theme-preview-description".into())
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .whitespace_normal()
                        .child(description),
                )
            })
            .child(
                Group::new("appearance_information")
                    .outline()
                    .child(
                        Entry::new("theme-info-author", tr("appearance_author"))
                            .control(value(license.author.clone())),
                    )
                    .child(
                        Entry::new(
                            "theme-info-version",
                            tr(if manifest.package_version.is_some() {
                                "settings_version"
                            } else {
                                "appearance_format_version"
                            }),
                        )
                        .control(value(
                            manifest
                                .package_version
                                .clone()
                                .unwrap_or_else(|| manifest.version.to_string()),
                        )),
                    )
                    .child(
                        Entry::new("theme-info-id", tr("appearance_identifier"))
                            .control(value(manifest.id.clone())),
                    ),
            )
            .child(
                Accordion::new("theme-license-details")
                    .w_full()
                    .h_auto()
                    // GPUI overflow masks are rectangular; paint the fill on Kit's rounded root.
                    .bg(cx.theme().tokens.accordion)
                    .item(|item| {
                        item.open(self.license_open)
                            .bg(cx.theme().transparent)
                            .title(
                                h_flex()
                                    .w_full()
                                    .min_w_0()
                                    .gap_3()
                                    .debug_selector(|| "theme-license-toggle".into())
                                    .child(tr("settings_license"))
                                    .when_some(license.name.clone(), |title, name| {
                                        title.child(
                                            div()
                                                .ml_auto()
                                                .min_w_0()
                                                .text_color(cx.theme().muted_foreground)
                                                .whitespace_normal()
                                                .child(name),
                                        )
                                    }),
                            )
                            .content_style(StyleRefinement::default().px_4().pb_4())
                            .child(
                                div()
                                    .debug_selector(|| "theme-license-text".into())
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .whitespace_normal()
                                    .child(license.text.clone()),
                            )
                    })
                    .on_toggle_click(cx.listener(|view, open: &[usize], _, cx| {
                        view.license_open = open.contains(&0);
                        cx.notify();
                    })),
            )
            .into_any_element()
    }
}
