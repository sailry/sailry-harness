use super::*;
use crate::{
    settings::group::{Group, Row},
    tr,
};
use gpui_kit::component::{input::Input, select::Select};

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.python(cx)
    }
}

impl Panel {
    fn python(&self, cx: &mut Context<Self>) -> Group {
        let locked = self.locked() || self.python.is_none();
        let group = Group::new("runtime_python").child(
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
        if !self.custom_index(cx) {
            return group;
        }
        group.child(
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
            .wide(),
        )
    }
}
