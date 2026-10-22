use super::*;
use sailry_protocol::plugin::{
    desktop::Navigation,
    ui::{Kind, Overflow},
};
use std::collections::BTreeMap;

fn core(id: &str, kind: Kind, align: Align, order: i16) -> Contribution {
    Contribution {
        intent: None,
        id: id.into(),
        slot: Slot::Composer,
        kind,
        label: Navigation {
            icon: None,
            label: id.into(),
            locales: BTreeMap::new(),
        },
        icon: None,
        handler: Some(format!("conversation.{id}")),
        order,
        overflow: Overflow::Auto,
        align,
        choices: vec![],
        command: None,
        width: None,
    }
}

pub(super) fn controls() -> Vec<Native> {
    vec![
        Native {
            declaration: Contribution {
                slot: Slot::Context,
                ..core("host", Kind::Select, Align::Start, -20)
            },
            toolbar: View::host_control,
            menu: View::host_control,
        },
        Native {
            declaration: core("mode", Kind::Select, Align::Start, 0),
            toolbar: |view, cx| (!view.sidebar).then(|| view.mode_menu(cx).into_any_element()),
            menu: |view, cx| (!view.sidebar).then(|| view.mode_entry(cx)),
        },
        Native {
            declaration: core("permission", Kind::Select, Align::Start, 10),
            toolbar: |view, cx| Some(view.permission_menu(cx).into_any_element()),
            menu: |view, cx| Some(view.permission_entry(cx)),
        },
    ]
}
