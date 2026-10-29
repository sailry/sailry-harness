use super::*;
use crate::{
    settings::group::{Group, Row},
    tr,
};
use gpui_kit::component::{input::Input, select::Select, spinner::Spinner, tag::Tag};
use gpui_kit::prelude::FluentBuilder as _;

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.python(cx)
    }
}

impl Panel {
    fn python(&self, cx: &mut Context<Self>) -> Group {
        let locked = self.locked() || self.python.is_none();
        let group = Group::new("runtime_python").child(self.version()).child(
            Row::new(
                "runtime_python_index",
                div()
                    .debug_selector(|| "runtime-python-source".into())
                    .w_full()
                    .min_w_0()
                    .child(
                        Select::new(&self.source)
                            .id("runtime-python-source")
                            .w_full()
                            .accessibility_label(tr("runtime_python_index"))
                            .disabled(locked),
                    ),
            )
            .wide(),
        );
        let group = if self.custom_index(cx) {
            group.child(self.custom_url(locked))
        } else {
            group
        };
        if self.preparations.is_empty() {
            return group;
        }
        group.child(Row::new(
            "runtime_python_preparing",
            v_flex()
                .debug_selector(|| "runtime-python-preparing".into())
                .items_end()
                .gap_1()
                .children(self.preparations.iter().map(|preparation| {
                    h_flex()
                        .min_w_0()
                        .gap_2()
                        .text_color(cx.theme().muted_foreground)
                        .child(Spinner::new().small())
                        .child(preparation.package.clone())
                        .child(crate::settings::python_stage(preparation.stage))
                })),
        ))
    }

    fn version(&self) -> Row {
        Row::new(
            "runtime_python_version",
            h_flex()
                .debug_selector(|| "runtime-python-version".into())
                .gap_2()
                .when_some(self.version.clone(), |row, version| {
                    row.child(version).child(
                        Tag::secondary()
                            .xsmall()
                            .child(tr("runtime_python_bundled")),
                    )
                }),
        )
    }

    fn custom_url(&self, locked: bool) -> Row {
        Row::new(
            "runtime_python_custom_url",
            div()
                .id("runtime-python-index")
                .debug_selector(|| "runtime-python-index".into())
                .w_full()
                .min_w_0()
                .child(
                    Input::new(&self.index)
                        .id("runtime-python-index-input")
                        .w_full()
                        .disabled(locked),
                ),
        )
        .wide()
    }
}
