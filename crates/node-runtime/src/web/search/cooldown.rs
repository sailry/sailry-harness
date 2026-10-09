use super::*;
use reqwest::header::{HeaderMap, RETRY_AFTER};
use std::time::UNIX_EPOCH;

pub(super) fn retry(headers: &HeaderMap, now: SystemTime) -> SystemTime {
    retry_value(
        headers
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok()),
        now,
    )
}

pub(super) fn retry_value(value: Option<&str>, now: SystemTime) -> SystemTime {
    let deadline = value.and_then(|value| {
        value
            .trim()
            .parse::<u64>()
            .ok()
            .and_then(|seconds| now.checked_add(Duration::from_secs(seconds)))
            .or_else(|| {
                chrono::DateTime::parse_from_rfc2822(value)
                    .ok()
                    .and_then(|date| {
                        u64::try_from(date.timestamp()).ok().and_then(|seconds| {
                            UNIX_EPOCH.checked_add(Duration::from_secs(seconds))
                        })
                    })
            })
    });
    deadline
        .unwrap_or_else(|| now + Duration::from_secs(60))
        .max(now)
}

pub(super) fn pending(deadline: SystemTime) -> Fault {
    let date: chrono::DateTime<chrono::Utc> = deadline.into();
    Fault::new(
        ErrorCode::Unavailable,
        format!(
            "search drivers are cooling down; retry at {}",
            date.to_rfc3339()
        ),
    )
}

impl Searcher {
    pub(super) fn cooldown(&self, driver: Driver, revision: u64) -> Option<SystemTime> {
        let mut state = self.state.lock().expect("search state lock is healthy");
        if state.settings.revision != revision {
            return None;
        }
        state
            .cooldowns
            .retain(|_, deadline| *deadline > SystemTime::now());
        state.cooldowns.get(&driver).copied()
    }

    pub(super) fn cool(&self, driver: Driver, revision: u64, deadline: SystemTime) {
        let mut state = self.state.lock().expect("search state lock is healthy");
        if state.settings.revision == revision {
            state.cooldowns.insert(driver, deadline);
        }
    }
}
