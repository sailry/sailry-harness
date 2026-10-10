//! Reply exports capture the pane's Node, session and turn before opening a picker.
use super::*;
use crate::live::menus::export::{self, Format, Selection};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};

impl View {
    pub(super) fn export_menu(&self, turn: TurnId, _cx: &mut Context<Self>) -> AnyElement {
        let selection = self.summary().cloned().map(|session| Selection {
            session,
            turn: Some(turn),
        });
        let binding = self.binding();
        Button::new(format!("reply-export-{turn}"))
            .debug_selector(move || format!("reply-export-{turn}"))
            .ghost()
            .small()
            .icon(IconName::EllipsisVertical)
            .tooltip(tr("more"))
            .accessibility_label(tr("more"))
            .disabled(selection.is_none() || !self.connected())
            .dropdown_menu(move |mut menu, _, _| {
                for (label, format) in [
                    ("conversation_export_markdown", Format::Markdown),
                    ("conversation_export_html", Format::Html),
                    ("conversation_export_image", Format::Image),
                ] {
                    let binding = binding.clone();
                    let selection = selection.clone();
                    menu = menu.item(
                        PopupMenuItem::new(tr(label))
                            .disabled(
                                format == Format::Image
                                    && !cfg!(any(target_os = "macos", target_os = "windows")),
                            )
                            .on_click(move |_, window, cx| {
                                if let Some(selection) = &selection {
                                    export::choose(
                                        binding.client.clone(),
                                        binding.runtime.clone(),
                                        selection.clone(),
                                        format,
                                        window,
                                        cx,
                                    );
                                }
                            }),
                    );
                }
                menu
            })
            .into_any_element()
    }
}
