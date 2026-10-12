use super::*;
use sailry_client::Client;
use sailry_link::{Admission, Pending, Subscription, Transport};
use sailry_protocol::{Fault, Request, Topic};
use std::sync::{Arc, Mutex};

struct Delayed {
    inner: Arc<dyn Transport>,
    held: Mutex<Vec<Held>>,
}

struct Held {
    result: Result<Output, Fault>,
    sender: tokio::sync::oneshot::Sender<Result<Output, Fault>>,
}

impl Transport for Delayed {
    fn target(&self) -> NodeId {
        self.inner.target()
    }

    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }

    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let held = matches!(
                request.command,
                Command::ReadBackupSettings | Command::ListBackups
            );
            let admission = self.inner.dispatch(request).await?;
            if !held {
                return Ok(admission);
            }
            assert!(!admission.receipt.durable);
            let result = admission
                .completion
                .await
                .expect("backup read completion expected");
            let (sender, completion) = tokio::sync::oneshot::channel();
            self.held.lock().unwrap().push(Held { result, sender });
            Ok(Admission {
                receipt: admission.receipt,
                completion,
            })
        })
    }
}

impl Delayed {
    fn complete(&self) {
        let Held { result, sender } = self.held.lock().unwrap().remove(0);
        sender.send(result).expect("backup read remains observed");
    }
}

fn delay(
    fixture: &Fixture,
    owner: &Entity<Workspace>,
    visual: &mut VisualTestContext,
) -> Arc<Delayed> {
    let delayed = Arc::new(Delayed {
        inner: fixture.transport.clone(),
        held: Mutex::default(),
    });
    visual.update(|_, cx| {
        owner.update(cx, |workspace, cx| {
            workspace.provider_link.as_mut().unwrap().binding.client =
                Arc::new(Client::new(delayed.clone()));
            workspace.select(super::super::super::Section::Backup, cx);
        });
    });
    delayed
}

#[gpui::test]
fn waits_for_settings_and_records(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let server = fixture
            .runtime
            .block_on(server::Server::start(Kind::Webdav));
        let mut saved = settings(&fixture);
        saved.destination = Some(server.destination());
        fixture.execute(Command::SaveBackupSettings(Update {
            settings: saved,
            identity: SecretUpdate::Replace(Secret::new("fixture-user".into())),
            secret: SecretUpdate::Replace(Secret::new("fixture-password".into())),
            session_token: SecretUpdate::Keep,
            passphrase: SecretUpdate::Replace(Secret::new("fixture-backup-passphrase".into())),
        }));
        fixture.execute(Command::ExportBackup {
            expected_revision: settings(&fixture).revision,
        });
        let (owner, visual) = fixture.mount(cx);
        let delayed = delay(&fixture, &owner, visual);
        wait(visual, |_| !delayed.held.lock().unwrap().is_empty());
        let panel = visual.update(|_, cx| selected(&owner, cx));
        wait(visual, |cx| panel.read(cx).refresh);
        assert!(visual.debug_bounds("backup-destination-loading").is_some());
        assert!(visual.debug_bounds("backup-destination-label").is_none());
        assert!(visual.debug_bounds("backup-records-loading").is_some());
        assert!(visual.debug_bounds("empty-backup_empty").is_none());
        delayed.complete();
        wait(visual, |_| !delayed.held.lock().unwrap().is_empty());
        assert!(visual.debug_bounds("backup-destination-loading").is_none());
        assert!(visual.debug_bounds("backup-destination-label").is_some());
        assert!(visual.debug_bounds("backup-records-loading").is_some());
        assert!(visual.debug_bounds("empty-backup_empty").is_none());
        delayed.complete();
        wait(visual, |cx| panel.read(cx).page.is_some());
        let record =
            visual.update(|_, cx| panel.read(cx).page.as_ref().unwrap().records[0].id.clone());
        let row: &'static str = Box::leak(format!("backup-row-{record}").into_boxed_str());
        let before = visual.debug_bounds(row).unwrap();
        // The initial Node snapshot coalesces one refresh behind the first reads.
        for _ in 0..2 {
            wait(visual, |_| !delayed.held.lock().unwrap().is_empty());
            assert_eq!(visual.debug_bounds(row), Some(before));
            assert!(visual.debug_bounds("backup-records-loading").is_none());
            delayed.complete();
        }
        wait(visual, |cx| !panel.read(cx).loading);
        assert_eq!(visual.debug_bounds(row), Some(before));
        visual.update(|window, cx| panel.update(cx, |panel, cx| panel.load(window, cx)));
        for _ in 0..2 {
            wait(visual, |_| !delayed.held.lock().unwrap().is_empty());
            assert_eq!(visual.debug_bounds(row), Some(before));
            assert!(visual.debug_bounds("backup-records-loading").is_none());
            delayed.complete();
        }
        wait(visual, |cx| !panel.read(cx).loading);
        assert_eq!(visual.debug_bounds(row), Some(before));
        fixture.close(visual);
    }
}

#[gpui::test]
fn failed_read_keeps_retry(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        let delayed = delay(&fixture, &owner, visual);
        wait(visual, |_| !delayed.held.lock().unwrap().is_empty());
        let panel = visual.update(|_, cx| selected(&owner, cx));
        wait(visual, |cx| panel.read(cx).refresh);
        for _ in 0..2 {
            wait(visual, |_| !delayed.held.lock().unwrap().is_empty());
            delayed.held.lock().unwrap()[0].result =
                Err(Fault::invalid("injected backup read failure"));
            delayed.complete();
        }
        wait(visual, |cx| !panel.read(cx).loading);
        assert!(visual.debug_bounds("backup-records-loading").is_none());
        assert!(visual.debug_bounds("empty-backup_empty").is_none());
        assert!(visual.debug_bounds("backup-load-retry").is_some());
        fixture.close(visual);
    }
}
