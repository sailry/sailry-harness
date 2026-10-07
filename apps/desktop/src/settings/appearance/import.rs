use super::super::entry::Entry;
use super::*;
use crate::theme::DialogStyle as _;
use gpui_kit::component::{
    accordion::Accordion,
    dialog::{DialogClose, DialogFooter},
    group_box::{GroupBox, GroupBoxVariants},
};
use gpui_kit::prelude::FluentBuilder as _;
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
    open(theme::Package::preview(&loaded), Some(loaded), window, cx);
}

pub(super) fn details(package: theme::Package, window: &mut Window, cx: &mut App) {
    open(package, None, window, cx);
}

fn open(package: theme::Package, loaded: Option<Loaded>, window: &mut Window, cx: &mut App) {
    let importing = loaded.is_some();
    let view = cx.new(|_| Import {
        package,
        loaded,
        busy: false,
        error: None,
        license_open: false,
    });
    view.update(cx, |_, cx| {
        crate::feedback::observe(window, cx, |view, _| view.error.into_iter().collect());
    });
    window.open_dialog(cx, move |dialog, window, cx| {
        let busy = view.read(cx).busy;
        let replacing = importing
            && cx
                .global::<theme::Catalog>()
                .packages
                .iter()
                .any(|package| package.id == view.read(cx).package.id);
        let save = view.clone();
        dialog
            .form_title(
                div()
                    .pr_6()
                    .min_w_0()
                    .truncate()
                    .debug_selector(|| "theme-preview-title".into())
                    .child(view.read(cx).package.name.clone()),
            )
            .w((window.viewport_size().width - px(48.)).min(px(640.)))
            .close_button(!busy)
            .keyboard(!busy)
            .overlay_closable(!busy)
            .child(view.clone())
            .footer(
                DialogFooter::new()
                    .w_full()
                    .child(
                        h_flex()
                            .flex_none()
                            // DialogClose fills its parent; constrain it to its trigger's width.
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
                    }),
            )
    });
}

struct Import {
    package: theme::Package,
    loaded: Option<Loaded>,
    busy: bool,
    error: Option<&'static str>,
    license_open: bool,
}

impl Import {
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
        let manifest = self
            .package
            .manifest
            .as_ref()
            .expect("imported theme manifest");
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
                            .fill()
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
                                    .child(theme::preview(&self.package, dark, cx)),
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
                    .item(|item| {
                        item.open(self.license_open)
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
    }
}
