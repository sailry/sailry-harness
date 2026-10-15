use super::*;
use crate::settings::group::Group;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::Input,
    list::List,
    select::Select,
};

impl Render for Library {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locked = !self.available(cx);
        let controls = h_flex()
            .w_full()
            .items_center()
            .gap_3()
            .debug_selector(|| "model-library-controls".into())
            .child(
                div()
                    .debug_selector(|| "model-library-source".into())
                    .w_48()
                    .flex_shrink_0()
                    .child(Select::new(&self.hub).w_full().disabled(locked)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .debug_selector(|| "model-library-query".into())
                    .child(
                        Input::new(&self.query)
                            .id("model-library-query")
                            .w_full()
                            .disabled(locked),
                    ),
            )
            .child(
                Button::new("model-library-search")
                    .ghost()
                    .small()
                    .icon(IconName::Search)
                    .accessibility_label(tr("model_library_search"))
                    .tooltip(tr("model_library_search"))
                    .debug_selector(|| "model-library-search".into())
                    .disabled(locked)
                    .on_click(
                        cx.listener(|library, _, window, cx| library.search(false, window, cx)),
                    ),
            );
        let mut results = Group::new("model_library_results")
            .heading(false)
            .empty(IconName::Search, "model_library_empty");
        if self.searched.is_none() {
            results = results.child(if self.loading {
                div().w_full().h_24().into_any_element()
            } else {
                div()
                    .h_24()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        crate::feedback::retry("model-library-retry")
                            .debug_selector(|| "model-library-retry".into())
                            .disabled(!self.available(cx))
                            .on_click(cx.listener(|library, _, window, cx| {
                                library.search(false, window, cx)
                            })),
                    )
                    .into_any_element()
            });
            if self.loading {
                results = results.overlay(crate::ui::pending::overlay("model-library-loading", cx));
            }
        }
        if !self.items.is_empty() {
            let height = (window.rem_size() * (self.items.len() as f32 * rows::ROW_HEIGHT))
                .min((window.viewport_size().height * 0.55).min(px(420.)));
            results = results.child(
                div()
                    .h(height)
                    .debug_selector(|| "model-library-list".into())
                    .child(List::new(&self.list).h_full()),
            );
        }
        v_flex()
            .w_full()
            .min_w_0()
            .gap_4()
            .child(controls)
            .child(results)
    }
}
