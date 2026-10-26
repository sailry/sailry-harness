//! Package requirements are compared with the execution Node's product version.
use sailry_protocol::{
    ErrorCode, Fault, FaultDetails,
    plugin::{Compatibility, Extension, Info},
};

pub(super) fn valid(required: Option<&str>) -> bool {
    required.is_none_or(|required| semver::Version::parse(required).is_ok())
}

fn compare(current: &str, required: Option<&str>) -> Compatibility {
    let Some(required) = required else {
        return Compatibility::Compatible;
    };
    let required_version = semver::Version::parse(required).expect("validated package requirement");
    let current = semver::Version::parse(current).expect("valid Sailry product version");
    if current.cmp_precedence(&required_version).is_lt() {
        Compatibility::RequiresNewer {
            required: required.into(),
        }
    } else {
        Compatibility::Compatible
    }
}

pub(super) fn evaluate(required: Option<&str>) -> Compatibility {
    compare(sailry_link::PRODUCT_VERSION, required)
}

pub(super) fn extension(extension: Option<&Extension>) -> Compatibility {
    evaluate(extension.and_then(|extension| extension.min_sailry_version.as_deref()))
}

pub(crate) fn refresh(info: &mut Info) {
    info.summary.compatibility = extension(info.extension.as_ref());
}

pub(crate) fn check(info: &Info) -> Result<(), Fault> {
    ensure(&extension(info.extension.as_ref()))
}

pub(super) fn ensure(compatibility: &Compatibility) -> Result<(), Fault> {
    match compatibility {
        Compatibility::Compatible => Ok(()),
        Compatibility::RequiresNewer { required } => {
            let mut error = Fault::new(
                ErrorCode::NotConfigured,
                "plugin requires a newer Sailry Node",
            );
            error.details = Some(FaultDetails::PluginRequiresNewer {
                required: required.clone(),
            });
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_semver_precedence() {
        for (current, required, compatible) in [
            ("0.1.0-alpha.4", "0.1.0-alpha.5", false),
            ("0.1.0-alpha.4", "0.1.0", false),
            ("0.1.0", "0.2.0-alpha.1", false),
            ("0.1.0-alpha.4+local", "0.1.0-alpha.4+published", true),
            ("0.1.0", "0.1.0-alpha.4", true),
        ] {
            assert_eq!(compare(current, Some(required)).is_compatible(), compatible);
        }
        assert!(compare("0.1.0-alpha.4", None).is_compatible());
        assert!(!valid(Some(">=0.1.0")));
    }
}
