//! Core settings entries use the existing sidebar groups and native rows.
use super::*;

impl Shell {
    pub(super) fn settings_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.settings.read(cx).section;
        let groups = Section::GROUPS
            .into_iter()
            .map(|(label, sections)| {
                let mut sections = sections.to_vec();
                sections.sort_by_key(|section| section.navigation_order());
                v_flex()
                    .gap_0p5()
                    .mb_3()
                    .child(Self::group_title(label, cx))
                    .children(sections.into_iter().map(|section| {
                        self.sidebar_row(
                            ("setting", section as usize),
                            Row::Setting(section),
                            selected == section
                                || selected == Section::Plugin && section == Section::Plugins,
                            cx,
                        )
                        .debug_selector(move || section.key().into())
                        .child(
                            h_flex()
                                .gap_2()
                                .child(if section == Section::Dictation {
                                    Icon::default().path("icons/reicon/microphone.svg")
                                } else {
                                    Icon::new(section.icon())
                                })
                                .child(tr(section.key())),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.clear_extension_details(cx);
                            this.settings
                                .update(cx, |settings, cx| settings.select(section, cx));
                            cx.notify();
                        }))
                    }))
            })
            .collect::<Vec<_>>();
        v_flex()
            .size_full()
            .text_color(cx.theme().sidebar_foreground)
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .debug_selector(|| "settings-navigation-viewport".into())
                    .child(
                        v_flex()
                            .id("settings-navigation")
                            .debug_selector(|| "settings-navigation".into())
                            .px_3()
                            .py_2()
                            .gap_0p5()
                            .overflow_y_scrollbar()
                            .children(groups),
                    ),
            )
    }
}
