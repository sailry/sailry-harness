use super::*;

pub(super) async fn search(
    endpoint: &str,
    auth: &Auth,
    input: &search::Request,
) -> Result<Vec<search::Result>, Failure> {
    let mut endpoint =
        url::Url::parse(endpoint).map_err(|_| Failure::invalid("SearXNG endpoint is invalid"))?;
    if endpoint.path().ends_with('/') {
        endpoint = endpoint
            .join("search")
            .map_err(|_| Failure::invalid("SearXNG endpoint is invalid"))?;
    }
    let mut parameters = vec![("q", filters::query(input)), ("format", "json".into())];
    if let Some(freshness) = input.freshness {
        // SearXNG accepts day/month/year; local filtering narrows widened ranges.
        parameters.push((
            "time_range",
            match freshness {
                search::Freshness::Hour | search::Freshness::Day => "day",
                search::Freshness::Week | search::Freshness::Month => "month",
                search::Freshness::Year => "year",
            }
            .into(),
        ));
    }
    let mut request = network::client()?.get(endpoint).query(&parameters);
    if let Some(key) = &auth.key {
        request = request.header("X-API-Key", key.expose());
    }
    if let Some(basic) = &auth.basic {
        let (user, password) = basic
            .expose()
            .split_once(':')
            .ok_or_else(Failure::unauthorized)?;
        request = request.basic_auth(user, Some(password));
    }
    parse(&network::json(request).await?)
}

pub(super) fn parse(value: &serde_json::Value) -> Result<Vec<search::Result>, Failure> {
    network::results(value, "/results", "title", "content", "publishedDate")
}
