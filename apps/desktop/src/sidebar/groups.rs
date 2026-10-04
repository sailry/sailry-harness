//! Split membership actions on the existing Kit resource rows.
use super::*;

impl Shell {
    pub(crate) fn split_folder(
        &self,
        entry: &tree::Entry,
        title: SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let target = entry.group.expect("split folder group");
        let open = self.sidebar.group_open(target);
        let disclosure = Button::new(SharedString::from(format!("split-disclosure-{target:?}")))
            .ghost()
            .xsmall()
            .icon(if open {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .accessibility_label(tr(if open {
                "workspace_collapse"
            } else {
                "workspace_expand"
            }))
            .debug_selector(move || format!("split-disclosure-{target:?}"))
            .on_click(cx.listener(move |shell, _, _, cx| {
                cx.stop_propagation();
                shell.sidebar.toggle_group(target);
                cx.notify();
            }));
        let row = self
            .sidebar_row(
                SharedString::from(format!("split-folder-{target:?}")),
                Row::Group(target),
                false,
                cx,
            )
            .pl(entry.indent())
            .debug_selector(move || format!("split-folder-{target:?}"))
            .on_drag(
                crate::panes::Drag::panel(target, title.clone()).payload(),
                |drag, _, _, cx| {
                    cx.new(|_| {
                        drag.value()
                            .downcast_ref::<crate::panes::Drag>()
                            .unwrap()
                            .clone()
                    })
                },
            )
            .child(entry.guides(format!("split-folder-{target:?}"), cx))
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .h_5()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .debug_selector(move || format!("split-folder-{target:?}-label"))
                            .child(title),
                    )
                    .child(disclosure),
            );
        self.split_destination(row, entry, cx)
            .on_click(cx.listener(move |shell, _, _, cx| {
                shell.sidebar.toggle_group(target);
                cx.notify();
            }))
            .into_any_element()
    }
}
