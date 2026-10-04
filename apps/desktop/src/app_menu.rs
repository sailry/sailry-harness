//! Native application menus use Kit's re-exported GPUI menu and action dispatch.
//! Standard entries follow Sailry Code 67ae9fa0's macOS MainMenu.xib; actions
//! reuse the current Shell, editing controls and window lifecycle.
use crate::{preview::Page, settings::Section, shell::Shell};
use gpui_kit::{component::input, prelude::*, *};

#[cfg(not(test))]
mod native;
#[cfg(feature = "workload-tests")]
mod native_check;
#[cfg(feature = "workload-tests")]
pub(crate) use native_check::run as verify_native;

actions!(
    sailry,
    [
        About,
        NewConversation,
        NewProject,
        OpenFolder,
        CloseWindow,
        Hide,
        HideOthers,
        ShowAll,
        Minimize,
        Zoom,
        FullScreen,
        BringAllToFront
    ]
);

#[derive(Default)]
struct Installed {
    navigation: Vec<Navigation>,
}
impl Global for Installed {}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Navigation {
    pub key: String,
    pub label: SharedString,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sailry, no_json)]
pub(crate) struct OpenFeature(pub String);

pub(crate) fn navigation(entries: Vec<Navigation>, cx: &mut App) {
    if cx
        .try_global::<Installed>()
        .is_none_or(|installed| installed.navigation == entries)
    {
        return;
    }
    cx.update_global::<Installed, _>(|installed, _| installed.navigation = entries);
    refresh(cx);
}

pub(crate) fn init(cx: &mut App) {
    if cx.has_global::<Installed>() {
        refresh(cx);
        return;
    }
    cx.set_global(Installed::default());
    shortcuts(cx);
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &BringAllToFront, cx| {
        let active = cx.active_window();
        for handle in cx
            .windows()
            .into_iter()
            .filter(|handle| Some(*handle) != active)
        {
            let _ = handle.update(cx, |_, window, _| window.activate_window());
        }
        if let Some(active) = active {
            let _ = active.update(cx, |_, window, _| window.activate_window());
        }
    });
    refresh(cx);
}

pub(crate) fn refresh(cx: &App) {
    #[cfg(not(test))]
    native::detach_services();
    let navigation = cx
        .try_global::<Installed>()
        .map(|installed| installed.navigation.as_slice())
        .unwrap_or_default();
    cx.set_menus(menus(&rust_i18n::locale(), navigation));
    #[cfg(not(test))]
    native::register_system_menus();
}

fn menus(locale: &str, navigation: &[Navigation]) -> Vec<Menu> {
    let label = |key: &str| rust_i18n::t!(key, locale = locale).to_string();
    vec![
        Menu::new(label("app")).items([
            MenuItem::action(label("menu_about"), About),
            MenuItem::separator(),
            MenuItem::action(
                label("menu_settings"),
                crate::shell::shortcuts::OpenPage(Page::Settings),
            ),
            MenuItem::separator(),
            MenuItem::os_submenu(label("menu_services"), SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action(label("menu_hide"), Hide),
            MenuItem::action(label("menu_hide_others"), HideOthers),
            MenuItem::action(label("menu_show_all"), ShowAll),
            MenuItem::separator(),
            MenuItem::action(label("menu_quit"), crate::shell::Quit),
        ]),
        Menu::new(label("menu_file")).items([
            MenuItem::action(label("menu_new_conversation"), NewConversation),
            MenuItem::action(label("menu_new_project"), NewProject),
            MenuItem::action(label("menu_open_folder"), OpenFolder),
            MenuItem::separator(),
            MenuItem::action(label("menu_save"), crate::resources::SaveFile),
            MenuItem::separator(),
            MenuItem::action(
                label("menu_close_tab"),
                crate::shell::shortcuts::CloseFocused,
            ),
            MenuItem::action(label("menu_close_window"), CloseWindow),
        ]),
        Menu::new(label("menu_edit")).items([
            MenuItem::os_action(label("menu_undo"), input::Undo, OsAction::Undo),
            MenuItem::os_action(label("menu_redo"), input::Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action(label("menu_cut"), input::Cut, OsAction::Cut),
            MenuItem::os_action(label("menu_copy"), input::Copy, OsAction::Copy),
            MenuItem::os_action(label("menu_paste"), input::Paste, OsAction::Paste),
            MenuItem::os_action(
                label("menu_select_all"),
                input::SelectAll,
                OsAction::SelectAll,
            ),
        ]),
        Menu::new(label("menu_view")).items(
            [
                MenuItem::action(label("menu_search"), crate::shell::Search),
                MenuItem::action(
                    label("menu_find_conversation"),
                    crate::shell::FindConversation,
                ),
                MenuItem::separator(),
                MenuItem::action(label("menu_toggle_sidebar"), crate::shell::ToggleSidebar),
                MenuItem::action(label("menu_toggle_details"), crate::shell::ToggleDetails),
                MenuItem::separator(),
                MenuItem::action(label("menu_full_screen"), FullScreen),
            ]
            .into_iter()
            .chain((!navigation.is_empty()).then_some(MenuItem::separator()))
            .chain(
                navigation.iter().map(|entry| {
                    MenuItem::action(entry.label.clone(), OpenFeature(entry.key.clone()))
                }),
            ),
        ),
        Menu::new(label("menu_window")).items([
            MenuItem::action(label("menu_minimize"), Minimize),
            MenuItem::action(label("menu_zoom"), Zoom),
            MenuItem::separator(),
            MenuItem::action(label("menu_bring_all_to_front"), BringAllToFront),
        ]),
    ]
}

fn shortcuts(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-alt-n", NewProject, Some("Sailry")),
        KeyBinding::new("cmd-o", OpenFolder, Some("Sailry")),
        KeyBinding::new("cmd-n", NewConversation, Some("Sailry")),
        KeyBinding::new("cmd-shift-w", CloseWindow, Some("Sailry")),
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("cmd-alt-h", HideOthers, None),
        KeyBinding::new("cmd-m", Minimize, Some("Sailry")),
        KeyBinding::new("ctrl-cmd-f", FullScreen, Some("Sailry")),
    ]);
}

pub(crate) fn bind(element: Stateful<Div>, cx: &Context<Shell>) -> Stateful<Div> {
    element
        .on_action(cx.listener(|shell, _: &About, window, cx| {
            shell.navigate(Page::Settings, window, cx);
            shell
                .settings
                .update(cx, |settings, cx| settings.select(Section::About, cx));
        }))
        .on_action(cx.listener(|shell, _: &NewConversation, window, cx| {
            shell.start_conversation(window, cx);
        }))
        .on_action(cx.listener(|shell, _: &NewProject, window, cx| {
            shell.project_editor(shell.host, None, window, cx);
        }))
        .on_action(cx.listener(|shell, _: &OpenFolder, window, cx| {
            shell.open_folder(window, cx);
        }))
        .on_action(cx.listener(Shell::open_feature))
        .on_action(cx.listener(Shell::close_window))
        .on_action(|_: &Minimize, window, _| window.minimize_window())
        .on_action(|_: &Zoom, window, _| window.zoom_window())
        .on_action(|_: &FullScreen, window, _| window.toggle_fullscreen())
}

#[cfg(test)]
mod tests;
