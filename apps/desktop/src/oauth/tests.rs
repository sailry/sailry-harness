use super::*;
use core::prelude::v1::test;
use gpui_kit::*;

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

#[gpui::test]
fn activates_an_existing_window(cx: &mut TestAppContext) {
    let (_, visual) = cx.add_window_view(|_, _| Empty);
    visual.deactivate_window();
    visual.update(|window, _| assert!(!window.is_window_active()));
    let (sender, receiver) = mpsc::unbounded();
    visual.update(|_, cx| super::init(receiver, cx));
    sender.unbounded_send(()).unwrap();
    visual.run_until_parked();
    visual.update(|window, cx| {
        assert!(window.is_window_active());
        assert_eq!(cx.windows().len(), 1);
    });
}

struct Empty;
impl Render for Empty {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
