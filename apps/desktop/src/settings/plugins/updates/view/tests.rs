use super::*;
use core::prelude::v1::test;

fn row(current: Option<&str>, candidate: Option<&str>) -> Row {
    let info: Info = serde_json::from_value(serde_json::json!({
        "summary": {
            "name": "example", "revision": 1, "settings_revision": 0,
            "digest": "candidate", "version": candidate,
        },
    }))
    .unwrap();
    let mut previous = info.summary.clone();
    previous.version = current.map(str::to_owned);
    Row {
        previous,
        changes: Changes::between(&info, &info),
        info,
        selected: true,
        expanded: false,
        status: Status::Ready,
    }
}

#[core::prelude::v1::test]
fn preserves_equal_versions() {
    assert_eq!(row(Some("0.1.0"), Some("0.1.0")).version(), "0.1.0 → 0.1.0");
}

#[core::prelude::v1::test]
fn preserves_current_and_candidate_order() {
    assert_eq!(row(Some("0.1.4"), Some("0.1.0")).version(), "0.1.4 → 0.1.0");
}

#[core::prelude::v1::test]
fn leaves_missing_versions_unknown() {
    assert_eq!(row(None, None).version(), "— → —");
    assert_eq!(row(None, Some("0.1.0")).version(), "— → 0.1.0");
    assert_eq!(row(Some("0.1.0"), None).version(), "0.1.0 → —");
}

#[gpui_kit::test]
fn uses_common_body_size(cx: &mut TestAppContext) {
    use crate::settings::plugins::test_support::{Fixture, draw, init};

    struct Frame(Entity<State>);
    impl Render for Frame {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            // Unit line height exposes the inherited font size without changing product layout.
            div().line_height(relative(1.)).child(self.0.clone())
        }
    }

    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (_, visual) = cx.add_window_view(|window, cx| {
            let state = cx.new(|_| State {
                binding: crate::settings::providers::Binding {
                    client: std::sync::Arc::new(sailry_client::Client::new(
                        fixture.transport.clone(),
                    )),
                    runtime: fixture.runtime.clone(),
                    label: "Fixture Node".into(),
                },
                packages: Vec::new(),
                entries: BTreeMap::new(),
                check: None,
                batch: None,
                task: None,
                dialog: Some(Selection {
                    rows: [("example".into(), row(Some("0.1.0"), Some("0.1.0")))].into(),
                    locked: false,
                    window: None,
                }),
                popup: Default::default(),
            });
            let frame = cx.new(|_| Frame(state));
            Root::new(frame, window, cx)
        });
        for (mode, rem_size, body_size) in [
            (ThemeMode::Light, 16., 14.),
            (ThemeMode::Dark, 16., 14.),
            (ThemeMode::Dark, 20., 17.5),
        ] {
            visual.update(|window, cx| {
                let theme = Theme::global_mut(cx);
                let config = match mode {
                    ThemeMode::Light => &mut theme.light_theme,
                    ThemeMode::Dark => &mut theme.dark_theme,
                };
                std::rc::Rc::make_mut(config).font_size = Some(rem_size);
                Theme::change(mode, Some(window), cx);
            });
            draw(visual);
            assert_eq!(visual.update(|window, _| window.rem_size()), px(rem_size));
            for selector in [
                "plugin-update-title-example",
                "plugin-update-version-example",
            ] {
                assert_eq!(
                    visual.debug_bounds(selector).unwrap().size.height,
                    px(body_size),
                    "{selector} must follow the shared body size"
                );
            }
        }
        fixture.close(visual);
    }
}
