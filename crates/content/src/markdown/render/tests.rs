use super::*;

mod visual_source {
    use super::*;

    #[test]
    fn preserves_document_behavior() {
        for (scenario, check) in [
            (
                "follows_selection_ends_in_editors",
                (|scenario: &'static str| {
                    let caret = Selection::at(Cursor::new(1, Part::Code, 0));
                    assert!(shows_source(true, Some(caret), 1), "{scenario}");
                    assert!(!shows_source(true, Some(caret), 2), "{scenario}");
                    assert!(!shows_source(false, Some(caret), 1), "{scenario}");
                    assert!(!shows_source(true, None, 1), "{scenario}");
                    let range = Selection::new(
                        Cursor::new(0, Part::Body, 0),
                        Cursor::new(3, Part::Body, 0),
                    );
                    assert!(shows_source(true, Some(range), 0), "{scenario}");
                    assert!(shows_source(true, Some(range), 3), "{scenario}");
                    assert!(!shows_source(true, Some(range), 2), "{scenario}");
                }) as fn(&'static str),
            ),
            (
                "places_caret_in_rendered_block",
                (|scenario: &'static str| {
                    let layouts = BlockLayouts::default();
                    layouts.record_visual(
                        2,
                        Bounds::new(point(px(0.), px(40.)), size(px(200.), px(80.))),
                    );
                    assert_eq!(
                        layouts.hit(point(px(10.), px(60.))),
                        Some(Cursor::new(2, Part::Code, 0)),
                        "{scenario}"
                    );
                    layouts.clear();
                    assert_eq!(layouts.hit(point(px(10.), px(60.))), None, "{scenario}");
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
