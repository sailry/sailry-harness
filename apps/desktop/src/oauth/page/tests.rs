use super::*;

#[test]
fn branding_and_copy() {
    for (scenario, check) in [
        (
            "renders_centered_local_branding",
            (|_scenario: &str| {
                let page = Page::new();
                let html = page.render(Outcome::Connected);
                assert!(
                    html.contains("data-outcome=\"connected\""),
                    "renders centered local branding"
                );
                assert!(
                    html.contains("display: grid; place-items: center"),
                    "renders centered local branding"
                );
                assert!(
                    html.contains("data:image/svg+xml;base64,"),
                    "renders centered local branding"
                );
                assert!(
                    html.contains("href=\"sailry://return\""),
                    "renders centered local branding"
                );
                assert!(
                    html.contains("history.replaceState(null, '', location.pathname)"),
                    "renders centered local branding"
                );
                assert!(
                    html.contains("window.close()"),
                    "renders centered local branding"
                );
                assert!(
                    html.contains(&escape(&page.titles[0])),
                    "renders centered local branding"
                );
                assert!(
                    !html.contains("https://"),
                    "renders centered local branding"
                );
            }) as fn(&str),
        ),
        (
            "escapes_localized_copy",
            (|_scenario: &str| {
                let page = Page {
                    language: "en\" unsafe".into(),
                    titles: std::array::from_fn(|_| "Signed in <safe> & \"quoted\"".into()),
                    descriptions: std::array::from_fn(|_| "Return <safely>".into()),
                    return_label: "Return <Sailry>".into(),
                    close_label: "Close </script>".into(),
                };
                let html = page.render(Outcome::Failed);
                assert!(
                    html.contains("lang=\"en&quot; unsafe\""),
                    "escapes localized copy"
                );
                assert!(
                    html.contains("Signed in &lt;safe&gt; &amp; &quot;quoted&quot;"),
                    "escapes localized copy"
                );
                assert!(
                    html.contains("Close &lt;/script&gt;"),
                    "escapes localized copy"
                );
                assert!(
                    html.contains("Return &lt;safely&gt;"),
                    "escapes localized copy"
                );
                assert!(!html.contains("Return <Sailry>"), "escapes localized copy");
                assert!(
                    html.contains("data-outcome=\"failed\""),
                    "escapes localized copy"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
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
