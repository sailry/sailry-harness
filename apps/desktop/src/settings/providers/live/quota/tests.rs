use super::*;
use core::prelude::v1::test;
use gpui_kit::component::{Root, WindowExt};
use sailry_link::{Admission, Pending, Subscription};
use sailry_protocol::{
    Authentication, CredentialId, CredentialRef, Output, Request, Snapshot, Topic,
    conversation::quota,
};
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

struct Counted {
    inner: Arc<dyn Transport>,
    calls: AtomicUsize,
}

impl Transport for Counted {
    fn target(&self) -> NodeId {
        self.inner.target()
    }

    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.dispatch(request)
    }

    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.subscribe(topic)
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn wait(cx: &mut VisualTestContext, predicate: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        draw(cx);
        if cx.update(|_, cx| predicate(cx)) {
            return;
        }
        assert!(Instant::now() < deadline, "quota presentation deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn project(owner: &Entity<Workspace>, cx: &mut VisualTestContext, snapshot: Snapshot) {
    owner.update(cx, |owner, cx| {
        owner.accept_providers(
            snapshot.node,
            View {
                snapshot: Some(snapshot),
                ..Default::default()
            },
            cx,
        );
    });
    draw(cx);
}

fn observation(snapshot: &mut Snapshot) -> quota::Snapshot {
    let provider = &mut snapshot.providers[0];
    provider.authentication = Authentication::ChatGpt;
    provider.api = ModelApi::Responses;
    provider.endpoint = "https://api.openai.com/v1".into();
    provider.credential = Some(CredentialRef {
        node: snapshot.node,
        id: CredentialId::new(),
    });
    quota::Snapshot {
        provider: provider.id,
        credential: provider.credential.clone().unwrap(),
        revision: 1,
        observed_ms: now(),
        five_hour: Some(quota::Window {
            used_basis_points: 4250,
            reset_at_ms: None,
        }),
        seven_day: Some(quota::Window {
            used_basis_points: 10050,
            reset_at_ms: None,
        }),
    }
}

fn baseline(
    fixture: &crate::settings::providers::fixture::Fixture,
    transport: Arc<dyn Transport>,
) -> Snapshot {
    let client = Client::new(transport);
    let Output::Snapshot(snapshot) = fixture
        .runtime
        .block_on(client.execute(client.prepare(Command::Snapshot)))
        .unwrap()
    else {
        panic!("Node snapshot expected")
    };
    snapshot
}

#[gpui::test]
fn keeps_geometry_and_expires_without_requests(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let fixture = crate::settings::providers::fixture::Fixture::new();
    for transport in &fixture.transports {
        let counted = Arc::new(Counted {
            inner: transport.clone(),
            calls: AtomicUsize::new(0),
        });
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let workspace = cx.new(|cx| Workspace::new(window, cx));
            workspace.update(cx, |workspace, cx| {
                workspace.bind_providers(
                    counted.clone(),
                    fixture.runtime.clone(),
                    "Quota Node".into(),
                    cx,
                );
                workspace.select(crate::settings::Section::Providers, cx);
            });
            owner = Some(workspace.clone());
            Root::new(workspace, window, cx)
        });
        let owner = owner.unwrap();
        wait(visual, |cx| {
            owner
                .read(cx)
                .provider_link
                .as_ref()
                .is_some_and(|live| live.connected && live.catalog.refresh.is_none())
        });
        owner.update(visual, |owner, _| {
            owner.provider_link.as_ref().unwrap().stop.cancel()
        });
        wait(visual, |cx| {
            !owner.read(cx).provider_link.as_ref().unwrap().connected
        });
        // Augment a real Node snapshot to isolate presentation; authenticated ingestion is covered by Node tests.
        let mut snapshot = baseline(&fixture, counted.clone());
        let mut quota = observation(&mut snapshot);
        project(&owner, visual, snapshot.clone());
        let geometry = [
            "provider-usage-0",
            "provider-plan-0",
            "provider-5h-0",
            "provider-7d-0",
        ]
        .map(|selector| visual.debug_bounds(selector).unwrap());
        assert!(visual.debug_bounds("provider-5h-progress-0").is_none());
        assert!(visual.debug_bounds("provider-7d-progress-0").is_none());

        let reset = now() + 1000;
        quota.five_hour.as_mut().unwrap().reset_at_ms = Some(reset);
        quota.seven_day.as_mut().unwrap().reset_at_ms = Some(reset);
        snapshot.provider_quotas = vec![quota.clone()];
        project(&owner, visual, snapshot.clone());
        for selector in ["provider-5h-progress-0", "provider-7d-progress-0"] {
            assert_eq!(visual.debug_bounds(selector).unwrap().size.height, px(4.));
        }
        assert!(visual.debug_bounds("provider-plan-progress-0").is_none());
        for (selector, before) in [
            "provider-usage-0",
            "provider-plan-0",
            "provider-5h-0",
            "provider-7d-0",
        ]
        .into_iter()
        .zip(geometry)
        {
            assert_eq!(visual.debug_bounds(selector).unwrap(), before);
        }
        owner.read_with(visual, |owner, _| {
            assert_eq!(owner.providers.channels[0].quota, Some(quota.clone()));
            assert_eq!(
                owner.provider_link.as_ref().unwrap().quota_reset,
                Some(reset)
            );
        });
        let calls = counted.calls.load(Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(reset.saturating_sub(now()) + 5));
        visual.executor().advance_clock(Duration::from_secs(2));
        draw(visual);
        assert!(visual.debug_bounds("provider-5h-progress-0").is_none());
        assert!(visual.debug_bounds("provider-7d-progress-0").is_none());
        for (selector, before) in [
            "provider-usage-0",
            "provider-plan-0",
            "provider-5h-0",
            "provider-7d-0",
        ]
        .into_iter()
        .zip(geometry)
        {
            assert_eq!(visual.debug_bounds(selector).unwrap(), before);
        }
        owner.read_with(visual, |owner, _| {
            assert_eq!(owner.providers.channels[0].quota, Some(quota.clone()));
            assert_eq!(owner.provider_link.as_ref().unwrap().quota_reset, None);
        });
        assert_eq!(
            counted.calls.load(Ordering::SeqCst),
            calls,
            "expiry must not fetch quota or reconnect"
        );
        assert!(visual.opened_url().is_none());
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
    }
    fixture.close();
}

#[gpui::test]
fn binds_current_account_and_clears_on_rebind(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let fixture = crate::settings::providers::fixture::Fixture::new();
    for (index, transport) in fixture.transports.iter().enumerate() {
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let workspace = cx.new(|cx| Workspace::new(window, cx));
            workspace.update(cx, |workspace, cx| {
                workspace.bind_providers(
                    transport.clone(),
                    fixture.runtime.clone(),
                    "Bound Node".into(),
                    cx,
                );
                workspace.select(crate::settings::Section::Providers, cx);
            });
            owner = Some(workspace.clone());
            Root::new(workspace, window, cx)
        });
        let owner = owner.unwrap();
        wait(visual, |cx| {
            owner
                .read(cx)
                .provider_link
                .as_ref()
                .is_some_and(|live| live.connected && live.catalog.refresh.is_none())
        });
        owner.update(visual, |owner, _| {
            owner.provider_link.as_ref().unwrap().stop.cancel()
        });
        wait(visual, |cx| {
            !owner.read(cx).provider_link.as_ref().unwrap().connected
        });
        let mut snapshot = baseline(&fixture, transport.clone());
        let quota = observation(&mut snapshot);
        let mut unselected = quota.clone();
        unselected.credential.id = CredentialId::new();
        let mut foreign = quota.clone();
        foreign.credential.node = fixture.transports[1 - index].target();
        snapshot.provider_quotas = vec![unselected.clone(), foreign.clone(), quota.clone()];
        project(&owner, visual, snapshot.clone());
        owner.read_with(visual, |owner, _| {
            assert_eq!(owner.providers.channels[0].quota, Some(quota.clone()))
        });
        assert!(visual.debug_bounds("provider-5h-progress-0").is_some());

        snapshot.provider_quotas = vec![unselected, foreign];
        project(&owner, visual, snapshot.clone());
        owner.read_with(visual, |owner, _| {
            assert_eq!(owner.providers.channels[0].quota, None)
        });
        assert!(visual.debug_bounds("provider-5h-progress-0").is_none());
        assert!(visual.debug_bounds("provider-7d-progress-0").is_none());

        snapshot.provider_quotas = vec![quota.clone()];
        project(&owner, visual, snapshot.clone());
        let mut foreign_snapshot = snapshot.clone();
        foreign_snapshot.node = fixture.transports[1 - index].target();
        foreign_snapshot.provider_quotas.clear();
        owner.update(visual, |owner, cx| {
            owner.accept_providers(
                snapshot.node,
                View {
                    snapshot: Some(foreign_snapshot),
                    ..Default::default()
                },
                cx,
            )
        });
        owner.read_with(visual, |owner, _| {
            assert_eq!(owner.providers.channels[0].quota, Some(quota.clone()))
        });

        snapshot.providers[0].credential.as_mut().unwrap().id = CredentialId::new();
        project(&owner, visual, snapshot);
        owner.read_with(visual, |owner, _| {
            assert_eq!(owner.providers.channels[0].quota, None)
        });
        assert!(visual.debug_bounds("provider-5h-progress-0").is_none());
        owner.update(visual, |owner, cx| {
            owner.bind_providers(
                fixture.transports[1 - index].clone(),
                fixture.runtime.clone(),
                "Next Node".into(),
                cx,
            )
        });
        wait(visual, |cx| {
            owner.read(cx).provider_link.as_ref().is_some_and(|live| {
                live.connected
                    && live.binding.client.target() == fixture.transports[1 - index].target()
            })
        });
        owner.read_with(visual, |owner, _| {
            assert!(
                owner
                    .providers
                    .channels
                    .iter()
                    .all(|channel| channel.quota.is_none())
            )
        });
        draw(visual);
        assert!(visual.debug_bounds("provider-5h-progress-0").is_none());
        assert!(visual.debug_bounds("provider-7d-progress-0").is_none());
    }
    fixture.close();
}
