use super::{Quit, Shell};
use crate::tr;
use gpui_kit::*;

#[derive(Clone, Copy)]
enum Exit {
    Application,
    Window,
}

#[derive(Default)]
struct Windows(Vec<(AnyWindowHandle, WeakEntity<Shell>)>);
impl Global for Windows {}

pub(crate) fn update_guard(cx: &App) -> Result<(), &'static str> {
    if cx.try_global::<Windows>().is_some_and(|windows| {
        windows.0.iter().any(|(_, owner)| {
            owner
                .read_with(cx, |shell, cx| shell.documents.any_unsaved(cx))
                .unwrap_or(false)
        })
    }) {
        return Err("updates_save_first");
    }
    Ok(())
}

pub(super) fn install(window: &mut Window, cx: &mut Context<Shell>) {
    let owner = cx.entity().downgrade();
    let handle = window.window_handle();
    if !cx.has_global::<Windows>() {
        cx.set_global(Windows::default());
        cx.on_window_closed(|cx, _| {
            let open = cx.windows();
            cx.global_mut::<Windows>()
                .0
                .retain(|(handle, _)| open.contains(handle));
        })
        .detach();
        App::on_action(cx, |_: &Quit, cx| {
            let windows = &cx.global::<Windows>().0;
            let selected = windows
                .iter()
                .find(|(handle, _)| Some(*handle) == cx.active_window())
                .or_else(|| {
                    windows
                        .iter()
                        .rev()
                        .find(|(_, owner)| owner.upgrade().is_some())
                })
                .cloned();
            let Some((handle, owner)) = selected else {
                return;
            };
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, cx| {
                    let _ = owner.update(cx, |shell, cx| {
                        shell.confirm_close(Exit::Application, window, cx)
                    });
                });
            });
        });
    }
    let windows = &mut cx.global_mut::<Windows>().0;
    windows.retain(|(_, owner)| owner.upgrade().is_some());
    windows.push((handle, owner.clone()));
    window.on_window_should_close(cx, move |window, cx| {
        let allowed = owner
            .update(cx, |shell, cx| {
                shell.confirm_close(Exit::Window, window, cx)
            })
            .unwrap_or(true);
        if allowed {
            release_browser(&owner, window, cx);
        }
        allowed
    });
}

fn release_browser(owner: &WeakEntity<Shell>, window: &mut Window, cx: &mut App) {
    let browsers = owner
        .update(cx, |shell, cx| {
            let cached = shell.browsers.close();
            shell
                .side_resource
                .iter()
                .chain(shell.retained_panels())
                .filter_map(|panel| panel.browser(cx))
                .chain(cached)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for browser in browsers {
        browser.update(cx, |browser, _| browser.close_all());
        window.refresh();
        window.draw(cx).clear(cx);
    }
}

impl Shell {
    #[cfg(target_os = "macos")]
    pub(crate) fn close_window(
        &mut self,
        _: &crate::app_menu::CloseWindow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.confirm_close(Exit::Window, window, cx) {
            let owner = cx.entity().downgrade();
            window.defer(cx, move |window, cx| {
                release_browser(&owner, window, cx);
                window.remove_window();
            });
        }
    }

    fn confirm_close(&mut self, exit: Exit, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let unsaved = self.documents.any_unsaved(cx)
            || matches!(exit, Exit::Application)
                && cx.global::<Windows>().0.iter().any(|(_, owner)| {
                    owner.entity_id() != cx.entity_id()
                        && owner
                            .read_with(cx, |shell, cx| shell.documents.any_unsaved(cx))
                            .unwrap_or(false)
                });
        if !unsaved && matches!(exit, Exit::Window) {
            return true;
        }
        if self.closing {
            return false;
        }
        self.closing = true;
        let owner = cx.entity().downgrade();
        let response = crate::prompts::ask(
            PromptLevel::Warning,
            &tr(if unsaved {
                "files_unsaved"
            } else {
                "app_exit_confirm"
            }),
            &tr(if unsaved {
                "files_exit_description"
            } else {
                "app_exit_description"
            }),
            &[
                tr("settings_cancel"),
                tr(if unsaved { "files_exit" } else { "app_exit" }),
            ],
            window,
            cx,
        );
        window
            .spawn(cx, async move |cx| {
                let confirmed = response.await == Some(1);
                let _ = cx.update(|window, cx| {
                    let _ = owner.update(cx, |shell, _| shell.closing = false);
                    if confirmed {
                        release_browser(&owner, window, cx);
                        match exit {
                            Exit::Application => {
                                let others = cx.global::<Windows>().0.clone();
                                for (handle, other) in others {
                                    if handle != window.window_handle() {
                                        let _ = handle.update(cx, |_, window, cx| {
                                            release_browser(&other, window, cx)
                                        });
                                    }
                                }
                                cx.quit();
                            }
                            Exit::Window => window.remove_window(),
                        }
                    }
                });
            })
            .detach();
        false
    }
}
