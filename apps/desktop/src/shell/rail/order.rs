//! Controller-owned feature order, shared by the rail and its menu.
use super::*;
use gpui_kit::component::list::ListItem;

pub(super) fn sort(features: &mut [Feature], cx: &App) {
    let order = crate::preferences::data(cx)
        .feature_order
        .unwrap_or_default();
    features.sort_by_key(|feature| {
        (
            feature.last(),
            order
                .iter()
                .position(|key| key == &feature.key)
                .unwrap_or(usize::MAX),
            feature.default_order,
        )
    });
}

pub(super) fn move_feature(
    features: &[Feature],
    source: &str,
    target: &str,
    after: bool,
    cx: &mut App,
) -> bool {
    if source == target
        || !features
            .iter()
            .any(|feature| feature.key == source && !feature.last())
    {
        return false;
    }
    let Some(destination) = features.iter().find(|feature| feature.key == target) else {
        return false;
    };
    let mut order = crate::preferences::data(cx)
        .feature_order
        .unwrap_or_default();
    // Keep temporarily unavailable contributions in their saved positions.
    for feature in features {
        if !order.contains(&feature.key) {
            order.push(feature.key.clone());
        }
    }
    // Management always trails built-in and external contributions.
    for feature in features.iter().filter(|feature| feature.last()) {
        order.retain(|key| key != &feature.key);
        order.push(feature.key.clone());
    }
    order.retain(|key| key != source);
    let position = order.iter().position(|key| key == target).unwrap()
        + usize::from(after && !destination.last());
    order.insert(position, source.to_owned());
    crate::preferences::update(cx, |data| data.feature_order = Some(order));
    cx.refresh_windows();
    true
}

#[derive(Clone)]
pub(super) struct Drag(pub Feature);

impl Render for Drag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ListItem::new("feature-drag-preview")
            .selected(true)
            .w_56()
            .h_9()
            .rounded(cx.theme().radius)
            .child(self.0.icon.clone().size_4())
            .child(div().truncate().child(self.0.label.clone()))
    }
}
