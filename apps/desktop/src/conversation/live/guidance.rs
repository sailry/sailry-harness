use crate::tr;
use gpui_kit::SharedString;
use sailry_client::conversation::guidance::State;

pub(super) fn label(state: State) -> SharedString {
    tr(match state {
        State::Pending => "guidance_pending",
        State::Delivered => "guidance_delivered",
        State::NextTurn => "guidance_next_turn",
    })
}
