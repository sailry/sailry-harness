use super::*;
use crate::agent::model::{Finish, Usage};
use async_trait::async_trait;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

enum Attempt {
    Reject(ErrorKind),
    Emit(Vec<Result<Event, Error>>),
    AwaitSetup,
    AwaitStream,
}

struct Fixture {
    attempts: Mutex<VecDeque<Attempt>>,
    calls: AtomicUsize,
    tokens: Mutex<Vec<CancellationToken>>,
}

impl Fixture {
    fn new(attempts: impl IntoIterator<Item = Attempt>) -> Arc<Self> {
        Arc::new(Self {
            attempts: Mutex::new(attempts.into_iter().collect()),
            calls: AtomicUsize::new(0),
            tokens: Mutex::new(Vec::new()),
        })
    }

    fn count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn token(&self) -> CancellationToken {
        self.tokens.lock().unwrap().last().unwrap().clone()
    }
}

#[async_trait]
impl Model for Fixture {
    async fn call(&self, _: Request, cancel: CancellationToken) -> Result<ModelStream, Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.tokens.lock().unwrap().push(cancel);
        let attempt = self.attempts.lock().unwrap().pop_front().unwrap();
        match attempt {
            Attempt::Reject(kind) => Err(failure(kind)),
            Attempt::Emit(events) => Ok(Box::pin(futures::stream::iter(events))),
            Attempt::AwaitSetup => futures::future::pending().await,
            Attempt::AwaitStream => Ok(Box::pin(futures::stream::pending())),
        }
    }
}

fn failure(kind: ErrorKind) -> Error {
    Error::new(kind, "fixture failure")
}

fn text(value: &str) -> Event {
    Event::TextDelta {
        index: 0,
        text: value.to_owned(),
    }
}

fn run(model: Arc<Fixture>, buffered: bool, retries: usize) -> ModelStream {
    stream(
        model,
        Request::default(),
        Policy {
            retries,
            delay: Duration::ZERO,
            buffered,
        },
        CancellationToken::new(),
        CancellationToken::new(),
    )
}

mod ordinary {
    use super::*;

    #[tokio::test]
    async fn allows_five_retries() {
        let model = Fixture::new((0..6).map(|_| Attempt::Reject(ErrorKind::Network)));
        let events = run(model.clone(), false, 5).collect::<Vec<_>>().await;
        assert_eq!(model.count(), 6);
        assert_eq!(events, vec![Err(failure(ErrorKind::Network))]);
    }

    #[tokio::test]
    async fn retries_stream_failure_before_output() {
        let model = Fixture::new([
            Attempt::Emit(vec![Err(failure(ErrorKind::Unavailable))]),
            Attempt::Emit(vec![Ok(text("completed")), Ok(Event::Finish(Finish::Stop))]),
        ]);
        let events = run(model.clone(), false, 5).collect::<Vec<_>>().await;
        assert_eq!(model.count(), 2);
        assert_eq!(
            events,
            vec![Ok(text("completed")), Ok(Event::Finish(Finish::Stop))]
        );
    }

    #[tokio::test]
    async fn keeps_failure_after_visible_output() {
        for output in [
            text("partial"),
            Event::Usage(Usage {
                output: Some(1),
                ..Usage::default()
            }),
        ] {
            let model = Fixture::new([Attempt::Emit(vec![
                Ok(output.clone()),
                Err(failure(ErrorKind::Network)),
            ])]);
            let events = run(model.clone(), false, 5).collect::<Vec<_>>().await;
            assert_eq!(model.count(), 1);
            assert_eq!(events, vec![Ok(output), Err(failure(ErrorKind::Network))]);
        }
    }

    #[tokio::test]
    async fn rejects_permanent_failures() {
        for kind in [
            ErrorKind::Authentication,
            ErrorKind::ContextOverflow,
            ErrorKind::Quota,
            ErrorKind::InvalidRequest,
            ErrorKind::InvalidResponse,
            ErrorKind::Unsupported,
            ErrorKind::Cancelled,
            ErrorKind::Other,
        ] {
            let model = Fixture::new([Attempt::Reject(kind)]);
            let events = run(model.clone(), false, 5).collect::<Vec<_>>().await;
            assert_eq!(model.count(), 1);
            assert_eq!(events, vec![Err(failure(kind))]);
        }
    }
}

mod compaction {
    use super::*;

    #[tokio::test]
    async fn discards_failed_output_and_usage() {
        let usage = Event::Usage(Usage {
            output: Some(100),
            ..Usage::default()
        });
        let model = Fixture::new([
            Attempt::Emit(vec![
                Ok(text("discard")),
                Ok(usage),
                Err(failure(ErrorKind::Network)),
            ]),
            Attempt::Emit(vec![Ok(text("summary")), Ok(Event::Finish(Finish::Stop))]),
        ]);
        let events = run(model.clone(), true, 1).collect::<Vec<_>>().await;
        assert_eq!(model.count(), 2);
        assert_eq!(
            events,
            vec![Ok(text("summary")), Ok(Event::Finish(Finish::Stop))]
        );
    }

    #[tokio::test]
    async fn stops_after_one_retry() {
        let model = Fixture::new((0..2).map(|_| {
            Attempt::Emit(vec![
                Ok(text("discard")),
                Err(failure(ErrorKind::RateLimited)),
            ])
        }));
        let events = run(model.clone(), true, 1).collect::<Vec<_>>().await;
        assert_eq!(model.count(), 2);
        assert_eq!(events, vec![Err(failure(ErrorKind::RateLimited))]);
    }
}

mod cancellation {
    use super::*;
    use futures::FutureExt;

    #[tokio::test]
    async fn cancels_setup_and_active_stream() {
        for attempt in [Attempt::AwaitSetup, Attempt::AwaitStream] {
            let model = Fixture::new([attempt]);
            let stop = CancellationToken::new();
            let mut responses = stream(
                model.clone(),
                Request::default(),
                Policy {
                    retries: 5,
                    delay: Duration::ZERO,
                    buffered: false,
                },
                CancellationToken::new(),
                stop.clone(),
            );
            assert!(responses.next().now_or_never().is_none());
            stop.cancel();
            assert_eq!(
                responses.next().await.unwrap().unwrap_err().kind,
                ErrorKind::Cancelled
            );
            assert!(model.token().is_cancelled());
            assert_eq!(model.count(), 1);
            assert!(responses.next().await.is_none());
        }
    }

    #[tokio::test]
    async fn stops_pending_retry() {
        let model = Fixture::new([Attempt::Reject(ErrorKind::Network)]);
        let cancel = CancellationToken::new();
        let mut responses = stream(
            model.clone(),
            Request::default(),
            Policy {
                retries: 5,
                delay: Duration::from_secs(60),
                buffered: false,
            },
            cancel.clone(),
            CancellationToken::new(),
        );
        assert!(responses.next().now_or_never().is_none());
        cancel.cancel();
        assert_eq!(
            responses.next().await.unwrap().unwrap_err().kind,
            ErrorKind::Cancelled
        );
        assert!(model.token().is_cancelled());
        assert_eq!(model.count(), 1);
    }

    #[tokio::test]
    async fn drop_cancels_model_token() {
        for attempt in [Attempt::AwaitSetup, Attempt::AwaitStream] {
            let model = Fixture::new([attempt]);
            let mut responses = run(model.clone(), false, 5);
            assert!(responses.next().now_or_never().is_none());
            let token = model.token();
            assert!(!token.is_cancelled());
            drop(responses);
            assert!(token.is_cancelled());
        }
    }
}
