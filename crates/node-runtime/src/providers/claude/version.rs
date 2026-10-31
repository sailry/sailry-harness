use std::sync::Mutex;

pub(super) const DEFAULT: &str = "2.1.280";
static ADOPTED: Mutex<Option<String>> = Mutex::new(None);

pub(crate) fn current() -> String {
    pinned()
        .or_else(|| ADOPTED.lock().expect("Claude version lock").clone())
        .unwrap_or_else(|| DEFAULT.into())
}

fn pinned() -> Option<String> {
    std::env::var("PI_AI_CLAUDE_CODE_VERSION")
        .ok()
        .filter(|value| !value.is_empty())
}

fn parts(value: &str) -> Option<[u64; 3]> {
    if value
        .split('.')
        .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    let values: Vec<_> = value
        .split('.')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    values.try_into().ok()
}

fn required(message: &str, current: &str) -> Option<String> {
    if !message.contains("claude_code_version_too_old") {
        return None;
    }
    let lower = message.to_ascii_lowercase();
    for (start, _) in lower.match_indices("version ") {
        let rest = &lower[start + 8..];
        let Some((value, suffix)) = rest.split_once(' ') else {
            continue;
        };
        if suffix.starts_with("or newer is required")
            && let Some(required) = parts(value)
            && required > parts(current)?
        {
            return Some(value.into());
        }
    }
    None
}

pub(crate) fn adopt(message: &str, sent: &str) -> bool {
    if pinned().is_some() {
        return false;
    }
    let mut adopted = ADOPTED.lock().expect("Claude version lock");
    update(message, sent, &mut adopted)
}

fn update(message: &str, sent: &str, adopted: &mut Option<String>) -> bool {
    let Some(next) = required(message, sent) else {
        return false;
    };
    if parts(&next) > parts(adopted.as_deref().unwrap_or(DEFAULT)) {
        *adopted = Some(next);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_concurrent_stale_versions() {
        let message = "claude_code_version_too_old: version 2.1.300 or newer is required";
        let mut adopted = None;
        assert!(update(message, DEFAULT, &mut adopted));
        assert!(update(message, DEFAULT, &mut adopted));
        assert_eq!(adopted.as_deref(), Some("2.1.300"));
        assert!(!update(message, "2.1.300", &mut adopted));
        assert!(update(
            "claude_code_version_too_old: version 2.1.290 or newer is required",
            DEFAULT,
            &mut adopted
        ));
        assert_eq!(adopted.as_deref(), Some("2.1.300"));
    }

    #[test]
    fn accepts_only_increasing_version_rejections() {
        let rejection = "claude_code_version_too_old: version 2.1.300 or newer is required";
        assert_eq!(required(rejection, DEFAULT).as_deref(), Some("2.1.300"));
        assert_eq!(
            required(
                "claude_code_version_too_old: version invalid or newer is required; version 2.1.300 or newer is required",
                DEFAULT,
            ).as_deref(),
            Some("2.1.300"),
        );
        assert!(required(rejection, "2.1.300").is_none());
        assert!(required(rejection, "2.2.0").is_none());
        assert!(required("version 2.1.300 or newer is required", DEFAULT).is_none());
        assert!(
            required(
                "claude_code_version_too_old: version invalid or newer is required",
                DEFAULT
            )
            .is_none()
        );
    }
}
