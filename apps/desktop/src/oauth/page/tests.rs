use super::*;

#[test]
fn renders_centered_local_branding() {
    let page = Page::new();
    let html = page.render(Outcome::Connected);
    assert!(html.contains("data-outcome=\"connected\""));
    assert!(html.contains("display: grid; place-items: center"));
    assert!(html.contains("data:image/svg+xml;base64,"));
    assert!(html.contains("href=\"sailry://return\""));
    assert!(html.contains("history.replaceState(null, '', location.pathname)"));
    assert!(html.contains("window.close()"));
    assert!(html.contains(&escape(&page.titles[0])));
    assert!(!html.contains("https://"));
}

#[test]
fn escapes_localized_copy() {
    let page = Page {
        language: "en\" unsafe".into(),
        titles: std::array::from_fn(|_| "Signed in <safe> & \"quoted\"".into()),
        return_label: "Return <Sailry>".into(),
        close_label: "Close </script>".into(),
    };
    let html = page.render(Outcome::Failed);
    assert!(html.contains("lang=\"en&quot; unsafe\""));
    assert!(html.contains("Signed in &lt;safe&gt; &amp; &quot;quoted&quot;"));
    assert!(html.contains("Close &lt;/script&gt;"));
    assert!(!html.contains("Return <Sailry>"));
    assert!(html.contains("data-outcome=\"failed\""));
}

#[tokio::test]
async fn reflects_only_the_confirmed_outcome() {
    for outcome in [Outcome::Connected, Outcome::Failed, Outcome::Cancelled] {
        let (sender, receiver) = oneshot::channel();
        let response = response(receiver);
        sender.send(outcome).unwrap();
        let Reply::Html(html) = response.await else {
            panic!("HTML page expected")
        };
        let state = match outcome {
            Outcome::Connected => "connected",
            Outcome::Failed => "failed",
            Outcome::Cancelled => "cancelled",
            Outcome::Pending => unreachable!(),
        };
        assert!(html.contains(&format!("data-outcome=\"{state}\"")));
    }
}

#[tokio::test]
async fn dropped_owner_does_not_report_success() {
    let (sender, receiver) = oneshot::channel();
    drop(sender);
    let Reply::Html(html) = response(receiver).await else {
        panic!("HTML page expected")
    };
    assert!(html.contains("data-outcome=\"pending\""));
    assert!(!html.contains("data-outcome=\"connected\""));
}
