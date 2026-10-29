use super::*;
use crate::settings::group::Group;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::Input,
    list::List,
    select::Select,
    spinner::Spinner,
};
use gpui_kit::prelude::FluentBuilder as _;

impl Render for Library {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.inline {
            return self.catalog(cx).into_any_element();
        }
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
        let mut results = self.pending(
            Group::new("model_library_results")
                .heading(false)
                .empty(IconName::Search, "model_library_empty"),
            cx,
        );
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
            .child(self.loading(results))
            .into_any_element()
    }
}

impl Library {
    /// Source and query controls an inline owner places beside its own tabs.
    pub(in crate::settings) fn controls(&self, locked: bool) -> Div {
        h_flex()
            .ml_auto()
            .items_center()
            .flex_wrap()
            .gap_2()
            .debug_selector(|| "model-library-controls".into())
            .child(
                div()
                    .debug_selector(|| "model-library-source".into())
                    .w(px(148.))
                    .flex_shrink_0()
                    .child(Select::new(&self.hub).w_full().small().disabled(locked)),
            )
            .child(
                div()
                    .w(px(260.))
                    .min_w_0()
                    .debug_selector(|| "model-library-query".into())
                    .child(
                        Input::new(&self.query)
                            .id("model-library-query")
                            .w_full()
                            .small()
                            .prefix(Icon::new(IconName::Search).small())
                            .disabled(locked),
                    ),
            )
    }

    /// Recommended entries followed by the current hub's results, in the owner's page.
    fn catalog(&mut self, cx: &mut Context<Self>) -> Div {
        let owner = cx.entity();
        let locked = !self.available(cx) || self.target.busy(cx);
        let unfiltered = self
            .searched
            .as_ref()
            .is_none_or(|(_, query)| query.is_empty());
        let recommended: Vec<ManagedModel> = if unfiltered {
            self.target
                .entries(cx)
                .into_iter()
                .filter(|model| !matches!(model.status, ModelStatus::Installed))
                .collect()
        } else {
            Vec::new()
        };
        let hub = self
            .searched
            .as_ref()
            .map_or_else(|| self.hub(cx), |(hub, _)| *hub);
        let mut results = self
            .pending(
                Group::new(match hub {
                    Hub::HuggingFace => "model_library_huggingface",
                    Hub::ModelScope => "model_library_modelscope",
                })
                .empty(IconName::Search, "model_library_empty"),
                cx,
            )
            .children((0..self.items.len()).filter_map(|row| rows::result(self, &owner, row, cx)));
        if self.has_more(cx) {
            results = results.child(
                h_flex().w_full().justify_center().py_2().child(
                    Button::new("model-library-more")
                        .ghost()
                        .small()
                        .label(tr("model_library_more"))
                        .debug_selector(|| "model-library-more".into())
                        .on_click(
                            cx.listener(|library, _, window, cx| library.search(true, window, cx)),
                        ),
                ),
            );
        }
        v_flex()
            .w_full()
            .min_w_0()
            .gap_6()
            .when(!recommended.is_empty(), |catalog| {
                catalog.child(
                    Group::new("local_models_recommended").children(managed::rows(
                        self.target.clone(),
                        recommended,
                        locked,
                        false,
                        cx,
                    )),
                )
            })
            .child(self.loading(results))
    }

    /// Placeholder or retry content before the first page of results arrives.
    fn pending(&self, results: Group, cx: &mut Context<Self>) -> Group {
        if self.loading && self.items.is_empty() {
            results.child(div().w_full().h_24())
        } else if self.searched.is_none() {
            results.child(
                div().h_24().flex().items_center().justify_center().child(
                    crate::feedback::retry("model-library-retry")
                        .debug_selector(|| "model-library-retry".into())
                        .disabled(!self.available(cx))
                        .on_click(
                            cx.listener(|library, _, window, cx| library.search(false, window, cx)),
                        ),
                ),
            )
        } else {
            results
        }
    }

    fn loading(&self, results: Group) -> Div {
        div()
            .relative()
            .w_full()
            .child(results)
            .when(self.loading, |card| {
                card.child(
                    div()
                        .id("model-library-loading")
                        .debug_selector(|| "model-library-loading".into())
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Spinner::new()),
                )
            })
    }
}
