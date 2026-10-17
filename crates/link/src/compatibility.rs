use std::cmp::Ordering;

use sailry_protocol::{ErrorCode, Fault, FaultDetails};
use semver::Version;

pub const PRODUCT_VERSION: &str = env!("SAILRY_PRODUCT_VERSION");

pub fn check_host(version: Option<&str>) -> Result<(), Fault> {
    check(Some(PRODUCT_VERSION), version)
}

pub(crate) fn check(controller: Option<&str>, host: Option<&str>) -> Result<(), Fault> {
    let local = controller.and_then(|version| Version::parse(version).ok());
    let remote = host.and_then(|version| Version::parse(version).ok());
    let unverified = local.is_none() || remote.is_none();
    let (update_host, message) = match (local, remote) {
        (Some(local), Some(remote)) => match local.cmp_precedence(&remote) {
            Ordering::Equal => return Ok(()),
            Ordering::Greater => (true, "Host is older than this controller; update Host"),
            Ordering::Less => (
                false,
                "Host is newer than this controller; update the controller",
            ),
        },
        (None, _) => (
            false,
            "Controller version could not be verified; update the controller",
        ),
        (_, None) => (true, "Host version could not be verified; update Host"),
    };
    let mut fault = Fault::new(ErrorCode::Conflict, message);
    fault.details = Some(FaultDetails::VersionMismatch {
        controller: controller.map(str::to_owned),
        host: host.map(str::to_owned),
        update_host,
        unverified,
    });
    Err(fault)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_matching_precedence() {
        assert!(check(Some("0.1.0-alpha.1+first"), Some("0.1.0-alpha.1+second")).is_ok());
        for (controller, host, update_host) in [
            (Some("0.1.0-alpha.2"), Some("0.1.0-alpha.1"), true),
            (Some("0.1.0-alpha.1"), Some("0.1.0-alpha.2"), false),
            (Some("1.0.0"), Some("0.9.0"), true),
            (Some("1.0.0"), Some("1.0.1"), false),
            (Some("1.0.0"), None, true),
            (Some("1.0.0"), Some("invalid"), true),
            (None, Some("1.0.0"), false),
            (Some("invalid"), Some("1.0.0"), false),
        ] {
            let error = check(controller, host).unwrap_err();
            assert_eq!(error.code, ErrorCode::Conflict);
            assert_eq!(
                error.details,
                Some(FaultDetails::VersionMismatch {
                    controller: controller.map(str::to_owned),
                    host: host.map(str::to_owned),
                    update_host,
                    unverified: controller.is_none_or(|value| Version::parse(value).is_err())
                        || host.is_none_or(|value| Version::parse(value).is_err()),
                })
            );
        }
    }
}
