use super::*;
use core::prelude::v1::test;

mod cells {
    use super::*;

    #[test]
    fn separators_and_whitespace() {
        for (case, input, expected) in [
            (
                "separator",
                "6WP-4T",
                vec![
                    Cell::Char('6'),
                    Cell::Char('W'),
                    Cell::Char('P'),
                    Cell::Gap,
                    Cell::Char('4'),
                    Cell::Char('T'),
                ],
            ),
            (
                "whitespace",
                " A B ",
                vec![Cell::Char('A'), Cell::Char('B')],
            ),
        ] {
            assert_eq!(cells(input), expected, "{case}");
        }
    }
}

mod host {
    use super::*;

    #[test]
    fn valid_and_invalid_urls() {
        for (input, expected) in [
            (
                "https://auth.openai.com/codex/device?x=1",
                Some("auth.openai.com"),
            ),
            ("https://www.github.com/login/device", Some("github.com")),
            ("not a url", None),
        ] {
            assert_eq!(host(input).as_deref(), expected, "{input}");
        }
    }
}

mod countdown {
    use super::*;

    #[test]
    fn duration_and_expiry() {
        for (case, deadline, now, expected) in [
            ("minutes", 872_000, 0, Some("14:32")),
            ("hours", 3_723_000, 0, Some("1:02:03")),
            ("partial second", 1_500, 1_000, Some("00:01")),
            ("at expiry", 1_000, 1_000, None),
            ("after expiry", 1_000, 2_000, None),
        ] {
            assert_eq!(countdown(deadline, now).as_deref(), expected, "{case}");
        }
    }
}
