//! One pane header composes conversation-scoped entry points.
use super::*;

pub(crate) struct Controls {
    search: Entity<search::Search>,
    assets: Entity<assets::Assets>,
}

impl Controls {
    pub(super) fn new(search: Entity<search::Search>, assets: Entity<assets::Assets>) -> Self {
        Self { search, assets }
    }
}

impl Render for Controls {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_1()
            .child(self.search.clone())
            .child(self.assets.clone())
    }
}
