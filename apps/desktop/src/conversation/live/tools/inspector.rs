//! Raw inspection is explicit and rechecks the captured conversation identity.
use super::*;
use crate::theme::DialogStyle as _;

impl View {
    pub(super) fn raw_action(&self, call: &Call, key: &str, cx: &mut Context<Self>) -> AnyElement {
        let source = call.response.clone();
        let name = call.name.clone();
        let id = call.id.clone();
        let node = self.binding.client.target();
        let turn = call.turn;
        let session = self
            .history
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.page.session);
        let key = format!("tool-raw-{key}");
        Button::new(key.clone())
            .ghost()
            .xsmall()
            .label(tr("tool_raw_result"))
            .on_click(cx.listener(move |view, _, window, cx| {
                if view.binding.client.target() != node {
                    return;
                }
                let Some(snapshot) = &view.history.snapshot else {
                    return;
                };
                if Some(snapshot.page.session) != session {
                    return;
                }
                let Some(source) = &source else { return };
                let result = snapshot
                    .page
                    .entries
                    .iter()
                    .find(|entry| entry.id == source.entry && entry.turn == turn)
                    .and_then(|entry| entry.parts.get(source.index))
                    .and_then(|part| match part {
                        sailry_protocol::conversation::Part::ToolResult {
                            id: actual_id,
                            name: actual_name,
                            result,
                            ..
                        } if *actual_id == id && *actual_name == name => Some(result),
                        _ => None,
                    });
                let Some(text) = result.map(output::raw) else {
                    return;
                };
                let text_size = px(f32::from(view.message_text_size));
                let key = key.clone();
                window.open_dialog(cx, move |dialog, _, cx| {
                    dialog
                        .when(!cx.theme().is_dark(), |dialog| dialog.floating_surface(cx))
                        .title(tr("tool_raw_result"))
                        .w(rems(40.).to_pixels(cx.theme().font_size))
                        .child(surface::result(
                            &key,
                            surface::code(&key, &text, text_size, cx),
                            [
                                surface::copy(format!("{key}-copy"), text.clone())
                                    .into_any_element(),
                            ],
                            cx,
                        ))
                });
            }))
            .into_any_element()
    }
}
