//! The local CLI account a connector binds, presented as a one-row selection list.
use super::*;
use crate::ui::modal_list::{List, Row};
use gpui_kit::component::{avatar::Avatar, skeleton::Skeleton};

// Kit's standard small avatar; custom pixel sizes do not center its initials.
const AVATAR_SIZE: f32 = 24.;

pub(super) fn list(identity: Option<&connector::Identity>) -> AnyElement {
    let list = List::new("connector-accounts").heading(tr("connector_choose_account"));
    match identity {
        Some(identity) => list
            .rows([Row::new("connector-account", identity.name.clone())
                .leading(Avatar::new().name(identity.name.clone()).small())
                .when(identity.id != identity.name, |row| {
                    row.subtitle(identity.id.clone())
                })
                .selected(true)])
            .into_any_element(),
        None => List::new("connector-accounts")
            .empty(tr("connector_not_connected"))
            .into_any_element(),
    }
}

/// Holds the account row's place while the Node inspects the local CLI.
pub(super) fn loading() -> AnyElement {
    let placeholder = h_flex()
        .gap_3()
        .items_center()
        .child(
            Skeleton::new()
                .size(px(AVATAR_SIZE))
                .flex_shrink_0()
                .rounded_full(),
        )
        .child(
            v_flex()
                .gap_1()
                .child(Skeleton::new().h_3().w(px(120.)).rounded_sm())
                .child(Skeleton::new().secondary().h_2p5().w(px(80.)).rounded_sm()),
        );
    List::new("connector-accounts")
        .rows([Row::new("connector-account-loading", "").leading(placeholder)])
        .into_any_element()
}
