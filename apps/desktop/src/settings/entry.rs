use gpui_kit::component::{
    button::{Button, ButtonVariants},
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// Settings records keep their independent controls outside the shrinking content slot.
#[derive(IntoElement)]
pub(super) struct Entry {
    id: SharedString,
    content: AnyElement,
    control: Option<AnyElement>,
    actions: Vec<Button>,
}

impl Entry {
    pub fn new(id: impl Into<SharedString>, content: impl IntoElement) -> Self {
        Self {
            id: id.into(),
            content: content.into_any_element(),
            control: None,
            actions: Vec::new(),
        }
    }

    pub fn control(mut self, control: impl IntoElement) -> Self {
        self.control = Some(control.into_any_element());
        self
    }

    pub fn action(mut self, button: Button) -> Self {
        self.actions.push(button.ghost().small());
        self
    }
}

impl RenderOnce for Entry {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let id = self.id.clone();
        let content_id = id.clone();
        let control_id = id.clone();
        let actions_id = id.clone();
        // Kit ListItem always highlights on hover; records only expose child controls.
        div()
            .id(self.id)
            .debug_selector(move || id.to_string())
            .w_full()
            .text_sm()
            .text_color(cx.theme().foreground)
            .px_0()
            .py_3()
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .child(
                        div()
                            .debug_selector(move || format!("{content_id}-content"))
                            .flex_1()
                            .min_w_0()
                            .child(self.content),
                    )
                    .when_some(self.control, |row, control| {
                        row.child(
                            h_flex()
                                .debug_selector(move || format!("{control_id}-control"))
                                .flex_shrink_0()
                                .child(control),
                        )
                    })
                    .when(!self.actions.is_empty(), |row| {
                        row.child(
                            h_flex()
                                .debug_selector(move || format!("{actions_id}-actions"))
                                .flex_shrink_0()
                                .gap_2()
                                .children(self.actions),
                        )
                    }),
            )
    }
}
