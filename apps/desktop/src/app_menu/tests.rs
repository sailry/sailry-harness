use super::*;
use core::prelude::v1::test;

fn labels(menus: &[Menu]) -> Vec<String> {
    let mut labels = Vec::new();
    for menu in menus {
        labels.push(menu.name.to_string());
        for item in &menu.items {
            match item {
                MenuItem::Action { name, .. } => labels.push(name.to_string()),
                MenuItem::SystemMenu(menu) => labels.push(menu.name.to_string()),
                MenuItem::Separator => {}
                MenuItem::Submenu(_) => panic!("unexpected nested application menu"),
            }
        }
    }
    labels
}

#[test]
fn localizes_every_entry() {
    let english = menus("en", &[]);
    assert_eq!(
        english
            .iter()
            .map(|menu| menu.name.as_ref())
            .collect::<Vec<_>>(),
        ["Sailry", "File", "Edit", "View", "Window", "Help"]
    );
    let english = labels(&english);
    let chinese = labels(&menus("zh-CN", &[]));
    assert_eq!(english.len(), chinese.len());
    assert_eq!(english[0], chinese[0]);
    for (english, chinese) in english.iter().zip(&chinese).skip(1) {
        assert_ne!(english, chinese, "untranslated menu entry");
        assert!(!english.starts_with("menu_"));
        assert!(!chinese.starts_with("menu_"));
    }
}

#[test]
fn registers_help_actions() {
    let menus = menus("en", &[]);
    let help = &menus[5];
    assert_eq!(help.items.len(), 3);
    for (item, expected) in help.items.iter().zip([
        Website.boxed_clone(),
        Feedback.boxed_clone(),
        Logs.boxed_clone(),
    ]) {
        let MenuItem::Action { action, .. } = item else {
            panic!("help action expected")
        };
        assert_eq!(action.as_any().type_id(), expected.as_any().type_id());
    }
    let MenuItem::Action { disabled, .. } = &help.items[2] else {
        panic!("logs action expected")
    };
    assert_eq!(
        *disabled,
        sailry_node_runtime::logging::directory().is_none()
    );
}

#[test]
fn uses_focused_editing_actions() {
    let menus = menus("en", &[]);
    let actions: Vec<_> = menus[2]
        .items
        .iter()
        .filter_map(|item| match item {
            MenuItem::Action {
                action, os_action, ..
            } => Some((action, os_action)),
            _ => None,
        })
        .collect();
    assert_eq!(actions.len(), 6);
    for ((action, os_action), expected) in actions.into_iter().zip([
        (input::Undo.boxed_clone(), OsAction::Undo),
        (input::Redo.boxed_clone(), OsAction::Redo),
        (input::Cut.boxed_clone(), OsAction::Cut),
        (input::Copy.boxed_clone(), OsAction::Copy),
        (input::Paste.boxed_clone(), OsAction::Paste),
        (input::SelectAll.boxed_clone(), OsAction::SelectAll),
    ]) {
        assert_eq!(action.as_any().type_id(), expected.0.as_any().type_id());
        assert!(*os_action == Some(expected.1));
    }
    assert!(menus[0].items.iter().any(|item| matches!(item, MenuItem::SystemMenu(menu) if menu.menu_type == SystemMenuType::Services)));
    assert!(menus[0].items.iter().any(|item| matches!(item, MenuItem::Action {action,..} if action.as_any().is::<crate::shell::Quit>())));
}
