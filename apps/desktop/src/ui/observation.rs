//! Multi-match access to Kit's recorded element snapshots for tests and workloads.

#[cfg(test)]
use gpui_kit::Role;
use gpui_kit::test::ElementSnapshot;
use gpui_kit::{ElementId, Window};

pub(crate) fn all(window: &Window, id: impl Into<ElementId>) -> Vec<ElementSnapshot> {
    let id = id.into();
    gpui_kit::base::test_support::snapshots(window)
        .into_iter()
        .filter(|snapshot| snapshot.path().last() == Some(&id))
        .collect()
}

pub(crate) fn within(
    window: &Window,
    scope: impl Into<ElementId>,
    id: impl Into<ElementId>,
) -> Vec<ElementSnapshot> {
    let scope = gpui_kit::base::test_support::scope(window, &[], &scope.into());
    let id = id.into();
    gpui_kit::base::test_support::snapshots(window)
        .into_iter()
        .filter(|snapshot| {
            snapshot.path().starts_with(&scope) && snapshot.path().last() == Some(&id)
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn roles(window: &Window, role: Role) -> Vec<ElementSnapshot> {
    roles_in(window, &[], role)
}

#[cfg(test)]
pub(crate) fn roles_within(
    window: &Window,
    scope: impl Into<ElementId>,
    role: Role,
) -> Vec<ElementSnapshot> {
    let scope = gpui_kit::base::test_support::scope(window, &[], &scope.into());
    roles_in(window, &scope, role)
}

#[cfg(test)]
fn roles_in(window: &Window, scope: &[ElementId], role: Role) -> Vec<ElementSnapshot> {
    let mut matches = gpui_kit::base::test_support::snapshots(window)
        .into_iter()
        .filter(|snapshot| {
            snapshot.path().len() > scope.len()
                && snapshot.path().starts_with(scope)
                && snapshot.role() == Some(role)
        })
        .collect::<Vec<_>>();
    // Kit records a map, so ordered menu assertions use the painted row positions.
    matches.sort_by(|left, right| {
        f32::from(left.bounds().top())
            .total_cmp(&f32::from(right.bounds().top()))
            .then_with(|| {
                f32::from(left.bounds().left()).total_cmp(&f32::from(right.bounds().left()))
            })
    });
    matches
}

#[cfg(test)]
pub(crate) fn label(window: &Window, label: &str) -> ElementSnapshot {
    try_label(window, label).unwrap_or_else(|| panic!("missing observed label {label:?}"))
}

#[cfg(test)]
pub(crate) fn try_label(window: &Window, label: &str) -> Option<ElementSnapshot> {
    label_in(window, &[], label)
}

#[cfg(test)]
pub(crate) fn label_within(
    window: &Window,
    scope: impl Into<ElementId>,
    label: &str,
) -> ElementSnapshot {
    let scope = gpui_kit::base::test_support::scope(window, &[], &scope.into());
    label_in(window, &scope, label)
        .unwrap_or_else(|| panic!("missing observed label {label:?} in scope {scope:?}"))
}

#[cfg(test)]
fn label_in(window: &Window, scope: &[ElementId], label: &str) -> Option<ElementSnapshot> {
    let mut matches = gpui_kit::base::test_support::snapshots(window)
        .into_iter()
        .filter(|snapshot| {
            snapshot.path().len() > scope.len()
                && snapshot.path().starts_with(scope)
                && snapshot.label() == Some(label)
        })
        .collect::<Vec<_>>();
    assert!(
        matches.len() <= 1,
        "ambiguous observed label {label:?} in scope {scope:?}: {matches:?}"
    );
    matches.pop()
}

#[cfg(test)]
mod tests {
    use super::{label, label_within, roles, roles_within, try_label};
    use gpui_kit::{
        AppContext, Context, IntoElement, Render, Role, Styled, TestAppContext, Window,
        component::Root,
        div,
        prelude::*,
        px,
        test::{TestSupportExt as _, TestWindowExt as _},
    };
    use std::panic::{AssertUnwindSafe, catch_unwind};

    struct Fixture;

    impl Render for Fixture {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let row = |id: &'static str, label: &'static str| {
                div()
                    .id(id)
                    .test_support()
                    .role(Role::Label)
                    .aria_label(label)
                    .w(px(100.))
                    .h(px(20.))
            };
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(
                    div()
                        .id("left")
                        .flex()
                        .flex_col()
                        .child(row("first", "Repeated"))
                        .child(row("second", "Only")),
                )
                .child(div().id("right").child(row("first", "Repeated")))
        }
    }

    #[gpui_kit::test]
    fn queries_preserve_scope_and_cardinality(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (_, visual) =
            cx.add_window_view(|window, cx| Root::new(cx.new(|_| Fixture), window, cx));
        visual.update(|window, cx| {
            window.render_frame(cx);
            assert_eq!(roles(window, Role::Label).len(), 3);
            let left = roles_within(window, "left", Role::Label);
            assert_eq!(
                left.iter().map(|item| item.label()).collect::<Vec<_>>(),
                vec![Some("Repeated"), Some("Only")]
            );
            assert!(left.iter().all(|item| item.visible()));
            assert_eq!(
                label_within(window, "left", "Repeated").path(),
                left[0].path()
            );
            assert_eq!(label(window, "Only").path(), left[1].path());
            assert!(try_label(window, "Missing").is_none());
            assert!(catch_unwind(AssertUnwindSafe(|| try_label(window, "Repeated"))).is_err());
        });
    }
}
