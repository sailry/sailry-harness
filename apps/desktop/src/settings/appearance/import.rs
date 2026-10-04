use super::*;
use crate::theme::DialogStyle as _;
use gpui_kit::component::scroll::ScrollableElement;
use std::path::PathBuf;
use theme::package::Loaded;

pub(super) fn choose(window: &mut Window, cx: &mut App) {
    let selected = cx.prompt_for_paths(PathPromptOptions {
        files: false,
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
    let view = cx.new(|_| Import {
        package: theme::Package::preview(&loaded),
        loaded: Some(loaded),
        busy: false,
        error: None,
    });
    view.update(cx, |_, cx| {
        crate::feedback::observe(window, cx, |view, _| view.error.into_iter().collect());
    });
    window.open_dialog(cx, move |dialog, window, cx| {
        let busy = view.read(cx).busy;
        let save = view.clone();
        dialog
            .form_title(tr("appearance_preview"))
            .w((window.viewport_size().width - px(48.)).min(px(640.)))
            .close_button(!busy)
            .keyboard(!busy)
            .overlay_closable(!busy)
            .child(view.clone())
            .footer(
                gpui_kit::component::dialog::DialogFooter::new()
                    .w_full()
                    .child(
                        Button::new("theme-import-confirm")
                            .primary()
                            .label(tr("appearance_import"))
                            .debug_selector(|| "theme-import-confirm".into())
                            .disabled(busy)
                            .on_click(move |_, window, cx| {
                                save.update(cx, |view, cx| view.save(window, cx))
                            }),
                    ),
            )
    });
}

struct Import {
    package: theme::Package,
    loaded: Option<Loaded>,
    busy: bool,
    error: Option<&'static str>,
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
                            catalog.add(loaded)?;
                            catalog.error = None;
                            Ok::<_, String>(())
                        });
                        if let Err(error) = result {
                            eprintln!("could not register imported theme: {error}");
                            fail("appearance_load_failed", cx);
                        }
                        window.close_dialog(cx);
                    }
                    Err(error) => {
                        eprintln!("could not save imported theme: {error}");
                        view.loaded = Some(loaded);
                        view.error = Some("appearance_save_failed");
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }
}

impl Render for Import {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let license = &self
            .package
            .manifest
            .as_ref()
            .expect("imported theme manifest")
            .license;
        v_flex()
            .gap_3()
            .child(div().text_lg().child(self.package.name.clone()))
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_3()
                    .children([false, true].map(|dark| {
                        v_flex()
                            .gap_2()
                            .child(theme::preview(&self.package, dark, cx))
                            .child(tr(if dark {
                                "appearance_dark"
                            } else {
                                "appearance_light"
                            }))
                    })),
            )
            .child(div().text_sm().child(license.author.clone()))
            .child(
                div()
                    .id("theme-license")
                    .max_h_24()
                    .overflow_y_scrollbar()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(license.text.clone()),
            )
    }
}
