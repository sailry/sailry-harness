use super::*;
use core::prelude::v1::test;

mod cells {
    use super::*;

    #[test]
    fn splits_separators_into_gaps() {
        assert_eq!(
            cells("6WP-4T"),
            vec![
                Cell::Char('6'),
                Cell::Char('W'),
                Cell::Char('P'),
                Cell::Gap,
                Cell::Char('4'),
                Cell::Char('T'),
            ]
        );
    }

    #[test]
    fn ignores_whitespace() {
        assert_eq!(cells(" A B "), vec![Cell::Char('A'), Cell::Char('B')]);
    }
}

mod host {
    use super::*;

    #[test]
    fn extracts_host_without_path() {
        assert_eq!(
            host("https://auth.openai.com/codex/device?x=1").as_deref(),
            Some("auth.openai.com")
        );
        assert_eq!(
            host("https://www.github.com/login/device").as_deref(),
            Some("github.com")
        );
    }

    #[test]
    fn rejects_invalid_urls() {
        assert_eq!(host("not a url"), None);
    }
}

mod countdown {
    use super::*;

    #[test]
    fn formats_minutes_and_hours() {
        assert_eq!(countdown(872_000, 0).as_deref(), Some("14:32"));
        assert_eq!(countdown(3_723_000, 0).as_deref(), Some("1:02:03"));
    }

    #[test]
    fn rounds_partial_seconds_up() {
        assert_eq!(countdown(1_500, 1_000).as_deref(), Some("00:01"));
    }

    #[test]
    fn stops_at_expiry() {
        assert_eq!(countdown(1_000, 1_000), None);
        assert_eq!(countdown(1_000, 2_000), None);
    }
}
