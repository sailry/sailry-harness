//! Monotonic execution intervals are reduced once by the existing turn owner.
use sailry_protocol::conversation::observation::Phases;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Phase {
    Waiting,
    Output,
    Tool,
    Excluded,
}

tokio::task_local! { static OWNER: u64; }

pub(crate) fn owner() -> Option<u64> {
    OWNER.try_with(|owner| *owner).ok()
}
pub(crate) async fn owned<T>(
    owner: Option<u64>,
    future: impl std::future::Future<Output = T>,
) -> T {
    match owner {
        Some(owner) => OWNER.scope(owner, future).await,
        None => future.await,
    }
}

type Interval = (u64, u64, Phase, Option<u64>);

pub(crate) struct Timeline {
    origin: Instant,
    spans: Mutex<Vec<Interval>>,
    next: AtomicU64,
    failures:
        Mutex<std::collections::BTreeMap<(String, String, String), sailry_protocol::tool::Failure>>,
}
pub(crate) struct Span {
    timeline: Arc<Timeline>,
    started: Instant,
    phase: Phase,
    owner: Option<u64>,
}
impl Timeline {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            origin: Instant::now(),
            spans: Mutex::new(Vec::new()),
            next: AtomicU64::new(1),
            failures: Mutex::new(Default::default()),
        })
    }
    pub(crate) fn span(self: &Arc<Self>, phase: Phase) -> Span {
        Span {
            timeline: self.clone(),
            started: Instant::now(),
            phase,
            owner: OWNER.try_with(|owner| *owner).ok(),
        }
    }
    pub(crate) async fn tool<T>(
        self: &Arc<Self>,
        delegated: bool,
        future: impl std::future::Future<Output = T>,
    ) -> T {
        let owner = self.next.fetch_add(1, Ordering::Relaxed);
        OWNER
            .scope(owner, async {
                let _span = self.span(if delegated {
                    Phase::Excluded
                } else {
                    Phase::Tool
                });
                future.await
            })
            .await
    }
    pub(crate) fn record(&self, start: Instant, elapsed: Duration, phase: Phase) {
        self.record_owned(start, elapsed, phase, None);
    }
    fn record_owned(&self, start: Instant, elapsed: Duration, phase: Phase, owner: Option<u64>) {
        let from = micros(start.saturating_duration_since(self.origin));
        self.spans
            .lock()
            .unwrap()
            .push((from, from.saturating_add(micros(elapsed)), phase, owner));
    }
    pub(crate) fn summary(&self) -> Phases {
        reduce(micros(self.origin.elapsed()), &self.spans.lock().unwrap())
    }

    pub(crate) fn fail_tool(
        &self,
        branch: &str,
        id: &str,
        name: &str,
        code: Option<sailry_protocol::ErrorCode>,
    ) {
        if !id.is_empty() {
            self.failures.lock().unwrap().insert(
                (branch.to_owned(), id.to_owned(), name.to_owned()),
                sailry_protocol::tool::Failure { code },
            );
        }
    }

    pub(crate) fn tool_failure(
        &self,
        branch: &str,
        id: &str,
        name: &str,
    ) -> Option<sailry_protocol::tool::Failure> {
        self.failures
            .lock()
            .unwrap()
            .get(&(branch.to_owned(), id.to_owned(), name.to_owned()))
            .cloned()
    }

    pub(crate) fn forget_failure(&self, branch: &str, id: &str, name: &str) {
        self.failures
            .lock()
            .unwrap()
            .remove(&(branch.to_owned(), id.to_owned(), name.to_owned()));
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        self.timeline
            .record_owned(self.started, self.started.elapsed(), self.phase, self.owner);
    }
}
fn micros(value: Duration) -> u64 {
    value.as_micros().try_into().unwrap_or(u64::MAX)
}
fn reduce(duration: u64, spans: &[Interval]) -> Phases {
    let mut points = vec![0, duration];
    for &(start, end, _, _) in spans {
        points.extend([start.min(duration), end.min(duration)]);
    }
    points.sort_unstable();
    points.dedup();
    let mut result = Phases {
        active_us: Some(0),
        waiting_us: Some(0),
        output_us: Some(0),
        tools_us: Some(0),
        other_us: Some(0),
    };
    for pair in points.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        let overlapping: Vec<_> = spans
            .iter()
            .copied()
            .filter(|&(from, to, _, _)| from < end && to > start)
            .collect();
        let phase = overlapping
            .iter()
            .filter(|&&(_, _, phase, owner)| {
                phase != Phase::Excluded
                    && !overlapping.iter().any(|&(_, _, other, excluded)| {
                        other == Phase::Excluded && excluded == owner
                    })
            })
            .map(|&(_, _, phase, _)| phase)
            .max();
        if phase.is_none()
            && overlapping
                .iter()
                .any(|&(_, _, phase, _)| phase == Phase::Excluded)
        {
            continue;
        }
        *result.active_us.as_mut().unwrap() += end - start;
        let segment = match phase {
            Some(Phase::Waiting) => &mut result.waiting_us,
            Some(Phase::Output) => &mut result.output_us,
            Some(Phase::Tool) => &mut result.tools_us,
            _ => &mut result.other_us,
        };
        *segment.as_mut().unwrap() += end - start;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn excludes_only_waiting_tool_owner() {
        let result = reduce(
            100,
            &[
                (0, 20, Phase::Waiting, None),
                (20, 40, Phase::Output, None),
                (40, 90, Phase::Tool, Some(1)),
                (50, 90, Phase::Tool, Some(2)),
                (60, 80, Phase::Excluded, Some(2)),
            ],
        );
        assert_eq!(
            result,
            Phases {
                active_us: Some(100),
                waiting_us: Some(20),
                output_us: Some(20),
                tools_us: Some(50),
                other_us: Some(10)
            }
        );
        let waiting = reduce(
            100,
            &[
                (0, 100, Phase::Tool, Some(1)),
                (20, 80, Phase::Excluded, Some(1)),
            ],
        );
        assert_eq!(waiting.active_us, Some(40));
        assert_eq!(waiting.tools_us, Some(40));
    }
}
