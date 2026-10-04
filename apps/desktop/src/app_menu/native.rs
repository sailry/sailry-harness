use objc2::{ClassType, MainThreadMarker, runtime::NSObjectProtocol};
use objc2_app_kit::{NSApplication, NSMenu};
use objc2_foundation::NSString;

pub(super) fn detach_services() {
    // Services cannot belong to two menus. Detach it while its current parent
    // is retained by the application, before Kit replaces the menu bar.
    let thread = MainThreadMarker::new().expect("native menus run on the main thread");
    let app = NSApplication::sharedApplication(thread);
    let Some(services) = app.servicesMenu() else {
        return;
    };
    let Some(bar) = app.mainMenu() else {
        return;
    };
    for item in bar.itemArray() {
        if let Some(menu) = item.submenu() {
            for item in menu.itemArray() {
                if item
                    .submenu()
                    .is_some_and(|menu| std::ptr::eq(&*menu, &*services))
                {
                    item.setSubmenu(None);
                    return;
                }
            }
        }
    }
}

pub(super) fn register_system_menus() {
    // gpui-pre-macos 0.3.4 registers the system window menu only when its
    // displayed title is "Window". Register the Kit-created submenu explicitly
    // so localized titles retain AppKit's window list and window management.
    let thread = MainThreadMarker::new().expect("native menus run on the main thread");
    let app = NSApplication::sharedApplication(thread);
    let Some(bar) = app.mainMenu() else {
        return;
    };
    let menu = bar
        .itemWithTitle(&NSString::from_str(&crate::tr("menu_window")))
        .and_then(|item| item.submenu());
    app.setWindowsMenu(menu.as_deref());
    // The same pinned backend passes the Services item instead of its submenu.
    // Keep Kit's menus and delegate, correcting only AppKit's system registration.
    let item = bar
        .itemWithTitle(&NSString::from_str(&crate::tr("app")))
        .and_then(|item| item.submenu())
        .and_then(|menu| menu.itemWithTitle(&NSString::from_str(&crate::tr("menu_services"))));
    if let Some(item) = item {
        // AppKit keeps its first Services submenu. Reattach it when Kit rebuilds
        // the menu bar for a locale or shortcut change.
        let services = app
            .servicesMenu()
            .filter(|menu| menu.isKindOfClass(NSMenu::class()))
            .or_else(|| item.submenu());
        item.setSubmenu(services.as_deref());
        app.setServicesMenu(services.as_deref());
    }
}
