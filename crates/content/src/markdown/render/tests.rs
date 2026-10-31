use super::*;

mod visual_source {
    use super::*;

    #[test]
    fn follows_selection_ends_in_editors() {
        let caret = Selection::at(Cursor::new(1, Part::Code, 0));
        assert!(shows_source(true, Some(caret), 1));
        assert!(!shows_source(true, Some(caret), 2));
        assert!(!shows_source(false, Some(caret), 1));
        assert!(!shows_source(true, None, 1));
        let range = Selection::new(Cursor::new(0, Part::Body, 0), Cursor::new(3, Part::Body, 0));
        assert!(shows_source(true, Some(range), 0));
        assert!(shows_source(true, Some(range), 3));
        assert!(!shows_source(true, Some(range), 2));
    }

    #[test]
    fn places_caret_in_rendered_block() {
        let layouts = BlockLayouts::default();
        layouts.record_visual(
            2,
            Bounds::new(point(px(0.), px(40.)), size(px(200.), px(80.))),
        );
        assert_eq!(
            layouts.hit(point(px(10.), px(60.))),
            Some(Cursor::new(2, Part::Code, 0))
        );
        layouts.clear();
        assert_eq!(layouts.hit(point(px(10.), px(60.))), None);
    }
}
