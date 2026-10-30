use super::*;

#[tokio::test]
#[ignore = "requires a ChatGPT subscription grant in SAILRY_SEARCH_SUBSCRIPTION_FILE"]
async fn searches_subscription_account() {
    let path = std::env::var_os("SAILRY_SEARCH_SUBSCRIPTION_FILE")
        .expect("subscription grant file must be supplied explicitly");
    let bytes = std::fs::read(path).expect("subscription grant file must be readable");
    let account: Value = serde_json::from_slice(&bytes).expect("subscription fixture must be JSON");
    let grant = crate::providers::login::Grant::decode(
        Authentication::ChatGptOAuth,
        &Secret::new(account["grant"].to_string()),
    )
    .expect("subscription grant must be valid");
    let model = account["model"]
        .as_str()
        .expect("subscription model must be supplied");
    let mut provider = auth::provider(Authentication::ChatGptOAuth, ModelApi::Responses);
    provider.endpoint = grant.endpoint().into();
    let credentials = Credentials::Grant {
        grant: Box::new(grant),
        options: sailry_protocol::conversation::oauth::Options::defaults(
            Authentication::ChatGptOAuth,
        )
        .unwrap(),
    };
    let prepared = prepare(
        &provider,
        &credentials,
        model,
        SessionId::new(),
        RequestId::new(),
        &search::Request {
            query: "What is the current stable Rust release? Verify using the official Rust blog"
                .into(),
            limit: 2,
            ..Default::default()
        },
        true,
    )
    .unwrap();
    let parsed = execute(
        &client(),
        prepared,
        &Run::new(&provider, model, None, CancellationToken::new()),
    )
    .await
    .unwrap();
    assert!(
        !parsed.results.is_empty(),
        "subscription search must return sources"
    );
    assert!(parsed.summary.as_ref().is_some_and(|text| !text.is_empty()));
    assert!(
        parsed
            .usage
            .as_ref()
            .is_some_and(|usage| usage.input.is_some())
    );
    println!(
        "subscription search returned {} sources with usage",
        parsed.results.len()
    );
}
