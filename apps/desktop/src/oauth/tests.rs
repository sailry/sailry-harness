use super::*;
use core::prelude::v1::test;

#[test]
fn accepts_only_credential_free_returns() {
    for url in [RETURN_URL, "sailry://return/"] {
        assert!(accepts(url));
    }
    for url in [
        "https://return",
        "sailry://another",
        "sailry://return/other",
        "sailry://return?code=secret",
        "sailry://return#token",
        "sailry://user@return",
        "sailry://return:80",
        "not a URL",
    ] {
        assert!(!accepts(url), "unexpected return link: {url}");
    }
}
