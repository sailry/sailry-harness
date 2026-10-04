use super::*;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
use gpui_kit::prelude::FluentBuilder as _;

impl View {
    pub(super) fn host_control(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let editable = self.can_retarget();
        let hosts = self.hosts.clone();
        let current_host = self.binding.client.target();
        let show_host = self.resource.is_none()
            && (hosts.len() > 1 || current_host != self.binding.defaults.target());
        if !show_host {
            return None;
        }
        let owner = cx.entity().downgrade();
        let host = Button::new("composer-host")
            .rounded_full()
            .ghost()
            .small()
            .icon(IconName::Cpu)
            .max_w_40()
            .min_w_0()
            .when(self.compact_composer, |button| button.flex_shrink_1())
            .when(!self.icon_context, |button| {
                button.label(self.binding.host.clone())
            })
            .when(self.icon_context, |button| {
                button.tooltip(self.binding.host.clone())
            })
            .accessibility_label(self.binding.host.clone())
            .dropdown_caret(editable && !self.icon_context)
            .disabled(!editable)
            .debug_selector(|| "composer-host".into());
        let host = host.dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
            hosts.iter().fold(menu, |menu, (node, label)| {
                let node = *node;
                let owner = owner.clone();
                menu.item(
                    PopupMenuItem::new(label.clone())
                        .checked(node == current_host)
                        .on_click(move |_, _, cx| {
                            let _ = owner.update(cx, |_, cx| cx.emit(Event::Host(node)));
                        }),
                )
            })
        });
        Some(host.into_any_element())
    }

    pub(super) fn context_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let form = if self.icon_context {
            crate::plugins::contributions::Form::Icons
        } else {
            crate::plugins::contributions::Form::Toolbar
        };
        let location = h_flex()
            .flex_1()
            .min_w_0()
            .gap_1()
            .when(!self.compact_composer, |row| row.flex_wrap())
            .children(self.registered_controls(
                sailry_protocol::plugin::ui::Slot::Context,
                sailry_protocol::plugin::ui::Align::Start,
                form,
                cx,
            ));
        let statistics = self.registered_statistics(cx);
        h_flex()
            .debug_selector(|| "composer-context-bar".into())
            .relative()
            .min_w_0()
            .mx_3()
            .px_2()
            .py_1()
            .gap_1()
            .when(!self.compact_composer, |bar| bar.flex_wrap())
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(location)
            .children(self.registered_controls(
                sailry_protocol::plugin::ui::Slot::Context,
                sailry_protocol::plugin::ui::Align::End,
                form,
                cx,
            ))
            .when(!statistics.is_empty(), |bar| {
                bar.child(if self.compact_composer {
                    super::super::usage::compact(statistics, cx)
                } else {
                    super::super::usage::render_groups(&statistics, cx)
                })
            })
            .into_any_element()
    }
}
