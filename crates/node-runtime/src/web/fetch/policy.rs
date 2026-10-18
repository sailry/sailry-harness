use reqwest::Url;
use sailry_protocol::{ErrorCode, Fault};
use std::net::{IpAddr, Ipv4Addr};

pub(super) fn denied() -> Fault {
    Fault::new(
        ErrorCode::PermissionDenied,
        "web fetch requires a public network address",
    )
}

pub(super) fn url(value: &str) -> Result<Url, Fault> {
    let mut url = Url::parse(value).map_err(|_| Fault::invalid("invalid web URL"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(Fault::invalid(
            "web URL must use HTTP(S) without credentials",
        ));
    }
    url.set_fragment(None);
    match url.host().expect("validated URL host") {
        url::Host::Ipv4(ip) if !public(IpAddr::V4(ip)) => return Err(denied()),
        url::Host::Ipv6(ip) if !public(IpAddr::V6(ip)) => return Err(denied()),
        url::Host::Domain(host) => {
            let host = host.trim_end_matches('.');
            if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
                return Err(denied());
            }
        }
        _ => {}
    }
    Ok(url)
}

pub(super) fn addresses(addresses: &[std::net::SocketAddr], local: &[IpAddr]) -> Result<(), Fault> {
    let normalize = |ip: IpAddr| match ip {
        IpAddr::V6(ip) => ip
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(ip)),
        ip => ip,
    };
    if addresses.is_empty()
        || addresses.iter().any(|address| {
            !public(address.ip())
                || local
                    .iter()
                    .any(|ip| normalize(*ip) == normalize(address.ip()))
        })
    {
        return Err(denied());
    }
    Ok(())
}

fn v4(ip: Ipv4Addr) -> bool {
    let [first, second, third, _] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_multicast()
        || first == 0
        || first >= 240
        || first == 100 && (64..=127).contains(&second)
        || first == 192 && second == 0 && (third == 0 || third == 2)
        || first == 192 && second == 88 && third == 99
        || first == 198 && (second == 18 || second == 19)
        || first == 198 && second == 51 && third == 100
        || first == 203 && second == 0 && third == 113)
}

pub(super) fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => v4(ip),
        IpAddr::V6(ip) => {
            if let Some(ip) = ip.to_ipv4_mapped() {
                return v4(ip);
            }
            let segments = ip.segments();
            segments[0] & 0xe000 == 0x2000
                && !(segments[0] == 0x2001 && segments[1] < 0x0200)
                && !(segments[0] == 0x2001 && segments[1] == 0x0db8)
                && segments[0] != 0x2002
                && !(segments[0] == 0x3fff && segments[1] & 0xf000 == 0)
        }
    }
}

#[cfg(test)]
mod tests;
