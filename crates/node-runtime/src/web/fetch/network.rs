use super::{Fetcher, cache::Document, html, policy};
use reqwest::{Url, header};
use sailry_protocol::{ErrorCode, Fault, web};
use std::{net::SocketAddr, time::Duration};

mod encoding;
mod proxy;
#[cfg(test)]
mod tests;

fn unavailable(message: &str) -> Fault {
    Fault::new(ErrorCode::Unavailable, message)
}

async fn lookup(_owner: &Fetcher, url: &Url) -> Result<Vec<SocketAddr>, Fault> {
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
        if fixture.resolved.is_empty() {
            return Err(unavailable("web hostname could not be resolved"));
        }
        return Ok(fixture
            .resolved
            .iter()
            .map(|ip| SocketAddr::new(*ip, port))
            .collect());
    }
    let addresses: Vec<_> = match url.host().expect("HTTP(S) URL host") {
        url::Host::Ipv4(ip) => vec![SocketAddr::new(ip.into(), port)],
        url::Host::Ipv6(ip) => vec![SocketAddr::new(ip.into(), port)],
        url::Host::Domain(_) => tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| unavailable("web hostname could not be resolved"))?
            .collect(),
    };
    Ok(addresses)
}

async fn local_addresses() -> Result<Vec<std::net::IpAddr>, Fault> {
    tokio::task::spawn_blocking(|| {
        sysinfo::Networks::new_with_refreshed_list()
            .values()
            .flat_map(|network| network.ip_networks().iter().map(|network| network.addr))
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|_| unavailable("local network addresses are unavailable"))
}

async fn resolve(owner: &Fetcher, url: &Url) -> Result<Vec<SocketAddr>, Fault> {
    let addresses = lookup(owner, url).await?;
    policy::addresses(&addresses, &local_addresses().await?)?;
    #[cfg(any(test, feature = "test-support"))]
    if let Some(fixture) = owner
        .fixtures
        .lock()
        .expect("web fixture lock is healthy")
        .get(url.host_str().expect("validated URL host"))
    {
        return Ok(vec![fixture.address]);
    }
    Ok(addresses)
}

async fn check_proxy(owner: &Fetcher, url: &Url) -> Result<(), Fault> {
    // A configured proxy resolves the destination. Local DNS is only a best-effort
    // check; fake-IP answers used by TUN proxies are not destination addresses.
    if let Ok(Ok(addresses)) =
        tokio::time::timeout(Duration::from_millis(500), lookup(owner, url)).await
    {
        let addresses: Vec<_> = addresses.into_iter().filter(|address| !matches!(address.ip(), std::net::IpAddr::V4(ip) if ip.octets()[0] == 198 && matches!(ip.octets()[1], 18 | 19))).collect();
        if !addresses.is_empty() {
            policy::addresses(&addresses, &local_addresses().await?)?;
        }
    }
    Ok(())
}

pub(super) async fn fetch(owner: &Fetcher, mut url: Url) -> Result<Document, Fault> {
    for redirects in 0..=web::MAX_REDIRECTS {
        let proxy = proxy::select(owner, &url).await?;
        let builder = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .referer(false)
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(10));
        let builder = if let Some(proxy) = proxy {
            check_proxy(owner, &url).await?;
            builder.proxy(proxy)
        } else {
            let addresses = resolve(owner, &url).await?;
            builder.resolve_to_addrs(url.host_str().expect("validated URL host"), &addresses)
        };
        #[cfg(any(test, feature = "test-support"))]
        let builder = owner.configure_fixture(&url, builder);
        let client = builder
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
        let declared_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("text/plain")
            .to_owned();
        let content_type = declared_type
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
                let body = encoding::decode(&body, &declared_type, html);
                if html {
                    html::convert(&body)
                } else {
                    Ok((None, body.into_owned()))
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
