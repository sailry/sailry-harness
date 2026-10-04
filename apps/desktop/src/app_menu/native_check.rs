//! Native AppKit acceptance uses preview state without a Node or user profile.
use super::*;
use objc2::{MainThreadMarker, rc::Retained};
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem};
use objc2_foundation::NSString;
use std::{path::Path, time::Duration};

pub(crate) fn run(output: &Path) {
    let output = output.to_owned();
    let actions = std::env::var("SAILRY_WORKLOAD_CASE").as_deref() == Ok("native-menu-actions");
    gpui_kit::application()
        .with_assets(crate::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
            rust_i18n::set_locale("en");
            crate::shell::init(cx);
            let output = output.clone();
            let options = crate::window_options(cx);
            cx.spawn(async move |cx| {
                cx.open_window(options, |window, cx| {
                    window.set_window_title("Sailry — native menu fixture");
                    window.activate_window();
                    let shell = cx.new(|cx| Shell::new(window, cx));
                    let probe = shell.clone();
                    window
                        .spawn(cx, async move |cx| {
                            if actions {
                                pause(cx).await;
                                assert!(
                                    cx.update(|_, cx| cx.active_window().is_some()).unwrap(),
                                    "native menu actions require an active macOS window"
                                );
                            }
                            for locale in ["en", "zh-CN"] {
                                cx.update(|_, cx| {
                                    rust_i18n::set_locale(locale);
                                    refresh(cx);
                                })
                                .unwrap();
                                pause(cx).await;
                                verify_menus(locale);
                                if actions {
                                    verify_actions(&probe, cx).await;
                                }
                            }
                            cx.update(|_, cx| {
                                crate::shortcuts::save(
                                    "app.search",
                                    Some("cmd-alt-shift-j"),
                                    cx,
                                )
                                .unwrap();
                            })
                            .unwrap();
                            let (_, item) = entry("menu_view", "menu_search");
                            assert_eq!(item.keyEquivalent().to_string(), "j");
                            assert!(
                                item.keyEquivalentModifierMask()
                                    .contains(NSEventModifierFlags::Command | NSEventModifierFlags::Shift | NSEventModifierFlags::Option)
                            );
                            verify_system_menus();
                            std::fs::write(
                                output.join("native-menus.json"),
                                serde_json::to_vec_pretty(&serde_json::json!({
                                    "appkit": true,
                                    "locales": ["en", "zh-CN"],
                                    "system_menus": true,
                                    "actions": actions,
                                    "clipboard": actions,
                                    "shortcut_refresh": true,
                                    "preview_only": true,
                                })).unwrap(),
                            )
                            .unwrap();
                            println!("Native menus: English, Chinese, system menus and shortcut refresh verified");
                            if actions {
                                println!("Native menu actions and clipboard verified");
                            }
                            cx.update(|_, cx| cx.quit()).unwrap();
                        })
                        .detach();
                    cx.new(|cx| gpui_kit::component::Root::new(shell, window, cx))
                })
                .unwrap();
                cx.update(|cx| cx.activate(true));
            })
            .detach();
        });
}

fn application() -> Retained<NSApplication> {
    NSApplication::sharedApplication(MainThreadMarker::new().unwrap())
}

fn submenu(key: &str) -> Retained<NSMenu> {
    application()
        .mainMenu()
        .unwrap()
        .itemWithTitle(&NSString::from_str(&crate::tr(key)))
        .unwrap()
        .submenu()
        .unwrap()
}

fn entry(menu: &str, key: &str) -> (Retained<NSMenu>, Retained<NSMenuItem>) {
    let menu = submenu(menu);
    let item = menu
        .itemWithTitle(&NSString::from_str(&crate::tr(key)))
        .unwrap();
    (menu, item)
}

fn verify_menus(locale: &str) {
    let bar = application().mainMenu().unwrap();
    let expected = menus(locale, &[]);
    assert_eq!(bar.numberOfItems() as usize, expected.len());
    for (index, menu) in expected.iter().enumerate() {
        let item = bar.itemAtIndex(index as isize).unwrap();
        assert_eq!(item.title().to_string(), menu.name.as_ref());
        assert!(item.submenu().is_some());
    }
    verify_system_menus();
    let (_, item) = entry("menu_file", "menu_new_conversation");
    assert_eq!(item.keyEquivalent().to_string(), "n");
    assert_eq!(
        item.keyEquivalentModifierMask(),
        NSEventModifierFlags::Command
    );
}

fn verify_system_menus() {
    let windows = submenu("menu_window");
    assert!(std::ptr::eq(
        &*application().windowsMenu().unwrap(),
        &*windows,
    ));
    let (_, item) = entry("app", "menu_services");
    let services = item.submenu().unwrap();
    assert!(std::ptr::eq(
        &*application().servicesMenu().unwrap(),
        &*services,
    ));
}

async fn invoke(menu: &str, key: &str, cx: &mut AsyncWindowContext) {
    // AppKit calls GPUI back synchronously; do not hold an App borrow here.
    let (menu, item) = entry(menu, key);
    menu.update();
    assert!(item.isEnabled(), "native menu action is disabled: {key}");
    menu.performActionForItemAtIndex(menu.indexOfItem(&item));
    pause(cx).await;
}

async fn pause(cx: &mut AsyncWindowContext) {
    cx.background_executor()
        .timer(Duration::from_millis(100))
        .await;
    cx.update(|window, cx| window.draw(cx).clear(cx)).unwrap();
}

async fn verify_actions(shell: &Entity<Shell>, cx: &mut AsyncWindowContext) {
    let clipboard = cx.update(|_, cx| cx.read_from_clipboard()).unwrap();
    let input = shell.update(cx, |shell, _| {
        assert!(shell.live.is_none());
        shell.conversations[&(shell.host, shell.session)]
            .input
            .clone()
    });
    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.set_value("Native menu draft", window, cx);
            input.focus(window, cx);
        });
    })
    .unwrap();
    pause(cx).await;
    invoke("menu_edit", "menu_select_all", cx).await;
    invoke("menu_edit", "menu_copy", cx).await;
    cx.update(|_, cx| {
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some("Native menu draft")
        );
    })
    .unwrap();
    invoke("menu_edit", "menu_cut", cx).await;
    assert_eq!(
        input.read_with(cx, |input, _| input.value().to_string()),
        ""
    );
    invoke("menu_edit", "menu_undo", cx).await;
    assert_eq!(
        input.read_with(cx, |input, _| input.value().to_string()),
        "Native menu draft"
    );
    invoke("menu_edit", "menu_redo", cx).await;
    invoke("menu_edit", "menu_paste", cx).await;
    assert_eq!(
        input.read_with(cx, |input, _| input.value().to_string()),
        "Native menu draft"
    );
    cx.update(|_, cx| {
        cx.write_to_clipboard(
            clipboard.unwrap_or_else(|| ClipboardItem::new_string(String::new())),
        );
    })
    .unwrap();
    invoke("app", "menu_about", cx).await;
    shell.read_with(cx, |shell, cx| {
        assert_eq!(shell.page, Page::Settings);
        assert_eq!(shell.settings.read(cx).section, Section::About);
    });
    let previous = shell.read_with(cx, |shell, _| (shell.host, shell.session));
    invoke("menu_file", "menu_new_conversation", cx).await;
    shell.read_with(cx, |shell, cx| {
        assert_eq!(shell.page, Page::Conversation);
        assert_ne!((shell.host, shell.session), previous);
        assert_eq!(input.read(cx).value(), "Native menu draft");
        assert!(!cx.has_global::<crate::backend::Services>());
    });
}
