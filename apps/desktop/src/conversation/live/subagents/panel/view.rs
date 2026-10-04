use super::*;
use crate::shell::Shell;
use gpui_kit::component::button::{Button, ButtonVariants};

impl Shell {
    pub(crate) fn live_child_panel(&self, panel: &Panel, cx: &mut Context<Self>) -> AnyElement {
        let source = panel.source.read(cx);
        let selected = panel.tabs.iter().position(|tab| tab.id == panel.selected);
        let items = panel
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let id = tab.id;
                let label = source
                    .child(id)
                    .map(|child| source.child_name(child))
                    .unwrap_or_else(|| tr("chat_child"));
                let close = Button::new(format!("close-child-{id}"))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Close)
                    .debug_selector(move || format!("close-child-{id}"))
                    .tooltip(tr("close"))
                    .accessibility_label(format!("{} {label}", tr("close")))
                    .on_click(cx.listener(move |shell, _, window, cx| {
                        cx.stop_propagation();
                        shell.close_live_child(id, window, cx);
                    }));
                let tab = crate::navigation_tabs::item(
                    format!("child-tab-{id}"),
                    label.clone(),
                    id == panel.selected,
                    Some(crate::ui::identicon::agent(&id.to_string(), cx).into_any_element()),
                    close,
                    cx,
                )
                .max_w(px(180.))
                .gap_1p5()
                .set_position(index + 1, panel.tabs.len())
                .on_click(cx.listener(move |shell, _, _, cx| shell.select_live_child(id, cx)));
                (label, tab.into_any_element())
            })
            .collect();
        v_flex()
            .debug_selector(|| "live-subagent-panel".into())
            .size_full()
            .min_h_0()
            .child(
                crate::header::Header::new("subagent-header", cx)
                    .bordered(false)
                    .px_2()
                    .child(crate::navigation_tabs::strip(
                        "child-tabs",
                        &panel.scroll,
                        items,
                        selected,
                        cx.listener(|shell, index: &usize, _, cx| {
                            if let Some(crate::resources::SideResource::Child(panel)) =
                                &shell.side_resource
                                && let Some(tab) = panel.tabs.get(*index)
                            {
                                shell.select_live_child(tab.id, cx);
                            }
                        }),
                        cx,
                    )),
            )
            .child(div().flex_1().min_h_0().child(panel.child().clone()))
            .into_any_element()
    }
}
