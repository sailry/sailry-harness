use std::cmp::Ordering;

use sailry_protocol::{ErrorCode, Fault, FaultDetails};

pub const PRODUCT_VERSION: &str = env!("SAILRY_PRODUCT_VERSION");

/// One peer's advertised build. Only the protocol number decides interoperability.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Peer<'a> {
    pub protocol: Option<u16>,
    pub version: Option<&'a str>,
}

impl Peer<'static> {
    pub(crate) const LOCAL: Self = Self {
        protocol: Some(sailry_protocol::VERSION),
        version: Some(PRODUCT_VERSION),
    };
}

impl<'a> Peer<'a> {
    pub(crate) fn of(handler: &'a dyn crate::Handler) -> Self {
        Self {
            protocol: handler.protocol(),
            version: handler.product_version(),
        }
    }
}

pub fn check_host(protocol: Option<u16>, version: Option<&str>) -> Result<(), Fault> {
    check(Peer::LOCAL, Peer { protocol, version })
}

/// Whether a Host's product release precedes this controller's. This offers
/// a Host update only; connection admission depends on [`check_host`].
pub fn host_outdated(version: Option<&str>) -> bool {
    older(PRODUCT_VERSION, version)
}

fn older(controller: &str, host: Option<&str>) -> bool {
    let parse = |version: &str| semver::Version::parse(version).ok();
    match (parse(controller), host.and_then(parse)) {
        (Some(controller), Some(host)) => host.cmp_precedence(&controller) == Ordering::Less,
        // An unknown release cannot be ordered; protocol checks still apply.
        _ => false,
    }
}

pub(crate) fn check(controller: Peer<'_>, host: Peer<'_>) -> Result<(), Fault> {
    let unverified = controller.protocol.is_none() || host.protocol.is_none();
    let (update_host, message) = match (controller.protocol, host.protocol) {
        (Some(local), Some(remote)) => match local.cmp(&remote) {
            Ordering::Equal => return Ok(()),
            Ordering::Greater => (true, "Host protocol is older; update Host"),
            Ordering::Less => (false, "Host protocol is newer; update the controller"),
        },
        (None, _) => (
            false,
            "Controller protocol could not be verified; update the controller",
        ),
        (_, None) => (true, "Host protocol could not be verified; update Host"),
    };
    let mut fault = Fault::new(ErrorCode::Conflict, message);
    fault.details = Some(FaultDetails::VersionMismatch {
        controller: controller.version.map(str::to_owned),
        host: host.version.map(str::to_owned),
        controller_protocol: controller.protocol,
        host_protocol: host.protocol,
        update_host,
        unverified,
    });
    Err(fault)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(protocol: Option<u16>, version: &str) -> Peer<'_> {
        Peer {
            protocol,
            version: Some(version),
        }
    }

    #[test]
    fn ignores_product_versions() {
        assert!(check(peer(Some(1), "0.1.0"), peer(Some(1), "0.1.2")).is_ok());
        assert!(check(peer(Some(1), "0.1.2"), peer(Some(1), "0.1.0")).is_ok());
        let unknown = Peer {
            protocol: Some(1),
            version: None,
        };
        assert!(check(peer(Some(1), "invalid"), unknown).is_ok());
    }

    #[test]
    fn orders_host_releases_by_precedence() {
        assert!(older("0.1.2", Some("0.1.0")));
        assert!(older("0.1.0", Some("0.1.0-alpha.3")));
        assert!(!older("0.1.0", Some("0.1.0+build")));
        assert!(!older("0.1.0", Some("0.1.2")));
        assert!(!older("0.1.0", Some("invalid")));
        assert!(!older("0.1.0", None));
        assert!(!older("invalid", Some("0.1.0")));
    }

    #[test]
    fn names_the_older_side() {
        for (controller, host, update_host) in [
            (Some(2), Some(1), true),
            (Some(1), Some(2), false),
            (Some(1), None, true),
            (None, Some(1), false),
        ] {
            let error = check(peer(controller, "0.1.2"), peer(host, "0.1.0")).unwrap_err();
            assert_eq!(error.code, ErrorCode::Conflict);
            assert_eq!(
                error.details,
                Some(FaultDetails::VersionMismatch {
                    controller: Some("0.1.2".into()),
                    host: Some("0.1.0".into()),
                    controller_protocol: controller,
                    host_protocol: host,
                    update_host,
                    unverified: controller.is_none() || host.is_none(),
                })
            );
        }
    }
}
