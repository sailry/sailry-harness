use super::*;
use hyper_util::client::proxy::matcher::Matcher;

#[tokio::test]
async fn reuses_effective_proxy_configuration() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let owner = Fetcher::default();
    let url = Url::parse("http://public.sailry.test/page").unwrap();
    owner.fixture(url.host_str().unwrap(), "127.0.0.1:32100".parse().unwrap());
    let mut clients = Clients::default();
    clients.get(&owner, &url, None).unwrap();
    clients
        .get(
            &owner,
            &Url::parse("https://other.sailry.test/page").unwrap(),
            None,
        )
        .unwrap();
    assert_eq!(clients.0.len(), 1);
    for credentials in ["first:secret", "first:secret", "second:secret"] {
        owner.fixture_proxy(
            url.host_str().unwrap(),
            Matcher::builder()
                .all(format!("http://{credentials}@127.0.0.1:32101"))
                .build(),
        );
        clients
            .get(&owner, &url, proxy::select(&owner, &url).await.unwrap())
            .unwrap();
    }
    assert_eq!(clients.0.len(), 3);
    owner.fixture_proxy(
        url.host_str().unwrap(),
        Matcher::builder()
            .all("http://first:secret@127.0.0.1:32101")
            .no("public.sailry.test")
            .build(),
    );
    clients
        .get(&owner, &url, proxy::select(&owner, &url).await.unwrap())
        .unwrap();
    assert_eq!(clients.0.len(), 3);
}

#[tokio::test]
async fn checks_connection_addresses_independently() {
    let owner = Fetcher::default();
    owner.fixture("first.sailry.test", "127.0.0.1:32100".parse().unwrap());
    owner.fixture("second.sailry.test", "127.0.0.1:32101".parse().unwrap());
    let resolver = Resolver::new(&owner);
    let (first, second) = tokio::join!(
        resolver.checked("first.sailry.test", 80),
        resolver.checked("second.sailry.test", 80)
    );
    assert_eq!(first.unwrap(), vec!["127.0.0.1:32100".parse().unwrap()]);
    assert_eq!(second.unwrap(), vec!["127.0.0.1:32101".parse().unwrap()]);
    owner.fixture_dns("first.sailry.test", vec!["127.0.0.1".parse().unwrap()]);
    assert!(resolver.checked("first.sailry.test", 80).await.is_err());
    assert_eq!(
        resolver.checked("second.sailry.test", 80).await.unwrap(),
        vec!["127.0.0.1:32101".parse().unwrap()]
    );
}
