use super::{Fetcher, cache::Document, html, policy};
use reqwest::{Url, header};
use sailry_protocol::{ErrorCode, Fault, web};
use std::{net::SocketAddr, time::Duration};

fn unavailable(message: &str) -> Fault {
    Fault::new(ErrorCode::Unavailable, message)
}

async fn resolve(_owner: &Fetcher, url: &Url) -> Result<Vec<SocketAddr>, Fault> {
    let port = url.port_or_known_default().expect("HTTP(S) URL port");
    let host = url.host_str().expect("HTTP(S) URL host");
    #[cfg(any(test, feature = "test-support"))]
    if let Some(fixture) = _owner
        .fixtures
        .lock()
        .expect("web fixture lock is healthy")
        .get(host)
        .cloned()
    {
        policy::addresses(
            &fixture
                .resolved
                .iter()
                .map(|ip| SocketAddr::new(*ip, port))
                .collect::<Vec<_>>(),
            &[],
        )?;
        return Ok(vec![fixture.address]);
    }
    let addresses: Vec<_> = match url.host().expect("HTTP(S) URL host") {
        url::Host::Ipv4(ip) => vec![SocketAddr::new(ip.into(), port)],
        url::Host::Ipv6(ip) => vec![SocketAddr::new(ip.into(), port)],
        url::Host::Domain(_) => tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| unavailable("web hostname could not be resolved"))?
            .collect(),
    };
    let local = tokio::task::spawn_blocking(|| {
        sysinfo::Networks::new_with_refreshed_list()
            .values()
            .flat_map(|network| network.ip_networks().iter().map(|network| network.addr))
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|_| unavailable("local network addresses are unavailable"))?;
    policy::addresses(&addresses, &local)?;
    Ok(addresses)
}

pub(super) async fn fetch(owner: &Fetcher, mut url: Url) -> Result<Document, Fault> {
    for redirects in 0..=web::MAX_REDIRECTS {
        let addresses = resolve(owner, &url).await?;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_proxy()
            .referer(false)
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(10))
            .resolve_to_addrs(url.host_str().expect("validated URL host"), &addresses)
            .build()
            .map_err(|_| unavailable("web HTTP client is unavailable"))?;
        let mut response = client
            .get(url.clone())
            .header(header::USER_AGENT, "Sailry/1")
            .header(
                header::ACCEPT,
                "text/html, application/json, text/plain, */*;q=0.5",
            )
            .header(header::ACCEPT_ENCODING, "identity")
            .send()
            .await
            .map_err(|_| unavailable("web request failed"))?;
        if matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308) {
            if redirects == web::MAX_REDIRECTS {
                return Err(unavailable("web redirect limit exceeded"));
            }
            let target = response
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| unavailable("web redirect has no valid location"))?;
            if target.len() > web::MAX_URL_BYTES {
                return Err(Fault::invalid("web redirect URL exceeds the size limit"));
            }
            let target = url
                .join(target)
                .map_err(|_| Fault::invalid("invalid web redirect URL"))?;
            url = policy::url(target.as_str())?;
            continue;
        }
        if response
            .content_length()
            .is_some_and(|size| size > web::MAX_BODY_BYTES as u64)
        {
            return Err(unavailable("web response exceeds the size limit"));
        }
        if response
            .headers()
            .get(header::CONTENT_ENCODING)
            .is_some_and(|value| value != "identity")
        {
            return Err(unavailable(
                "web response uses an unsupported content encoding",
            ));
        }
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("text/plain")
            .split(';')
            .next()
            .unwrap_or("text/plain")
            .trim()
            .to_ascii_lowercase();
        let html = matches!(content_type.as_str(), "text/html" | "application/xhtml+xml");
        let pdf = content_type == "application/pdf";
        if !html
            && !pdf
            && !content_type.starts_with("text/")
            && content_type != "application/json"
            && !content_type.ends_with("+json")
            && content_type != "application/xml"
            && !content_type.ends_with("+xml")
        {
            return Err(Fault::new(
                ErrorCode::Unavailable,
                format!("unsupported web content type: {content_type}"),
            ));
        }
        let status = response.status().as_u16();
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| unavailable("web response was interrupted"))?
        {
            if body.len().saturating_add(chunk.len()) > web::MAX_BODY_BYTES {
                return Err(unavailable("web response exceeds the size limit"));
            }
            body.extend_from_slice(&chunk);
        }
        let (title, content) = tokio::task::spawn_blocking(move || {
            if pdf {
                let sections = crate::office::pdf::read(&body)?;
                let content = sections
                    .into_iter()
                    .map(|(name, text)| format!("## {name}\n\n{text}"))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                Ok((None, content))
            } else {
                let body = String::from_utf8(body)
                    .map_err(|_| unavailable("web response is not UTF-8 text"))?;
                if html {
                    html::convert(&body)
                } else {
                    Ok((None, body))
                }
            }
        })
        .await
        .map_err(|_| unavailable("web content extraction failed"))??;
        if content.len() > web::MAX_BODY_BYTES {
            return Err(unavailable("web content exceeds the size limit"));
        }
        return Ok(Document {
            url: url.into(),
            status,
            content_type,
            content,
            title,
            fetched: std::time::Instant::now(),
        });
    }
    unreachable!("bounded redirect loop returns a page or error")
}
