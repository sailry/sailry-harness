use super::*;
use crate::settings::Section;
use crate::settings::knowledge::tests::fixture::wait_for;
use sailry_link::{Admission, Pending, Subscription, Transport};
use sailry_protocol::{Fault, RequestId, Topic};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

type Outcomes = Vec<(RequestId, Result<Output, Fault>)>;

struct Recording {
    inner: Arc<dyn Transport>,
    requests: Mutex<Vec<Request>>,
    outcomes: Arc<Mutex<Outcomes>>,
}

impl Transport for Recording {
    fn target(&self) -> NodeId {
        self.inner.target()
    }

    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }

    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let install = matches!(request.command, Command::InstallOcrModel { .. });
            if install || matches!(request.command, Command::InspectRequest { .. }) {
                self.requests.lock().unwrap().push(request.clone());
            }
            let mut admission = self.inner.dispatch(request).await?;
            if install {
                let outcomes = self.outcomes.clone();
                let id = admission.receipt.id;
                let (sender, receiver) = tokio::sync::oneshot::channel();
                let completion = std::mem::replace(&mut admission.completion, receiver);
                tokio::spawn(async move {
                    if let Ok(result) = completion.await {
                        outcomes.lock().unwrap().push((id, result.clone()));
                        _ = sender.send(result);
                    }
                });
            }
            Ok(admission)
        })
    }
}

impl Recording {
    fn new(inner: Arc<dyn Transport>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            requests: Mutex::default(),
            outcomes: Arc::default(),
        })
    }

    fn installs(&self) -> Vec<Request> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| matches!(request.command, Command::InstallOcrModel { .. }))
            .cloned()
            .collect()
    }

    fn fault(&self, attempt: usize) -> Fault {
        self.outcomes.lock().unwrap()[attempt]
            .1
            .as_ref()
            .unwrap_err()
            .clone()
    }
}

fn open<'a>(
    fixture: &Fixture,
    recording: Arc<Recording>,
    model: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<Panel>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        cx.set_global(crate::preferences::Preferences::open(
            fixture.directory.path().join("preferences.json"),
        ))
    });
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let workspace = cx.new(|cx| Workspace::new(window, cx));
        workspace.update(cx, |workspace, cx| {
            // Bind once: rebinding the same Node retains the existing Client.
            workspace.bind_providers(
                recording,
                fixture.runtime.clone(),
                "OCR fixture host".into(),
                cx,
            );
            workspace.select(Section::Ocr, cx);
        });
        owner = Some(workspace.clone());
        Root::new(workspace, window, cx)
    });
    let owner = owner.unwrap();
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(960.), px(1100.)));
    wait(visual, |cx| {
        owner.read(cx).runtime.panels.values().any(|panel| {
            let panel = panel.read(cx);
            !panel.loading && panel.models.iter().any(|entry| entry.id == model)
        })
    });
    let panel = visual.update(|_, cx| {
        owner
            .read(cx)
            .runtime
            .panels
            .get(&fixture.transport.target())
            .unwrap()
            .clone()
    });
    draw(visual);
    let available = visual.debug_bounds("runtime-tab-available").unwrap();
    visual.simulate_click(available.center(), Modifiers::default());
    draw(visual);
    assert_eq!(visual.update(|_, cx| panel.read(cx).tab), 1);
    (panel, visual)
}

fn assert_failure(
    recording: &Recording,
    panel: &Entity<Panel>,
    model: &str,
    attempt: usize,
    visual: &mut VisualTestContext,
) -> Fault {
    wait_for(
        visual,
        |cx| {
            let panel = panel.read(cx);
            !panel.pending && panel.fault.is_some() && recording.installs().len() == attempt + 1
        },
        |_, cx| {
            format!(
                "OCR failure attempt {attempt} deadline: {}",
                state(recording, panel, cx)
            )
        },
    );
    let fault = recording.fault(attempt);
    visual.update(|_, cx| {
        let panel = panel.read(cx);
        assert_eq!(panel.fault.as_ref(), Some(&fault));
        assert!(
            panel.request.is_none(),
            "confirmed failure must release the request"
        );
        assert_eq!(panel.retry_install.as_deref(), Some(model));
        assert_eq!(panel.binding.client.target(), recording.target());
        assert!(panel.active);
        assert!(panel.settings.as_ref().unwrap().model.is_none());
        assert!(
            panel
                .models
                .iter()
                .all(|entry| entry.state != Status::Installed)
        );
    });
    crate::feedback::tests::shown(visual);
    crate::feedback::tests::settle(visual);
    assert!(visual.debug_bounds("error-toast-detail").is_some());
    visual.update(|window, cx| {
        assert_eq!(window.notifications(cx).len(), 1);
        assert_eq!(
            window.find("error-detail").label(),
            Some(fault.message.as_str())
        );
        assert_eq!(
            window.find("runtime-retry").label(),
            Some(crate::tr("chat_retry").as_ref())
        );
        assert_eq!(
            crate::feedback::tests::summary(window, cx),
            crate::tr("runtime_failed")
        );
    });
    let request = recording.installs()[attempt].clone();
    assert_eq!(
        request.command,
        Command::InstallOcrModel { id: model.into() }
    );
    let install = request.id;
    assert!(recording.requests.lock().unwrap().iter().any(|request| {
        matches!(request.command, Command::InspectRequest { id, .. } if id == install)
    }));
    fault
}

fn state(recording: &Recording, panel: &Entity<Panel>, cx: &App) -> String {
    let panel = panel.read(cx);
    let models = panel
        .models
        .iter()
        .map(|model| (&model.id, &model.state))
        .collect::<Vec<_>>();
    format!(
        "active={}, pending={}, loading={}, request={:?}, retry_install={:?}, fault={:?}, models={models:?}, installs={:?}, outcomes={:?}",
        panel.active,
        panel.pending,
        panel.loading,
        panel
            .request
            .as_ref()
            .map(|request| (request.id, &request.command)),
        panel.retry_install,
        panel.fault,
        recording.installs(),
        recording.outcomes.lock().unwrap(),
    )
}

fn blocked_path(fixture: &Fixture, id: &str) -> std::path::PathBuf {
    let directory = fixture.directory.path().join("node/models/ocr").join(id);
    std::fs::write(&directory, "storage blocker").unwrap();
    directory
}

fn assert_identity(recording: &Recording, model: &str) {
    let requests = recording.installs();
    assert_eq!(requests.len(), 2);
    assert_ne!(requests[0].id, requests[1].id);
    assert_eq!(requests[0].command, requests[1].command);
    assert_eq!(
        requests[1].command,
        Command::InstallOcrModel { id: model.into() }
    );
}

fn models(fixture: &Fixture) -> Vec<Model> {
    let Output::OcrModels(models) = fixture.execute(Command::ReadOcrModels) else {
        panic!("OCR models expected")
    };
    models
}

#[gpui::test]
fn retries_confirmed_install_with_fresh_identity(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let mut definition = models(&fixture)[0].definition.clone();
        definition.name = "Unvalidated fixture".into();
        for asset in &mut definition.assets {
            let bytes = seed(&asset.name);
            asset.bytes = bytes.len() as u64;
            asset.sha256 = format!("{:x}", Sha256::digest(bytes));
        }
        let Output::OcrModel { id } = fixture.execute(Command::RegisterOcrModel {
            definition: definition.clone(),
        }) else {
            panic!("OCR registration expected")
        };
        let directory = blocked_path(&fixture, &id);
        let recording = Recording::new(fixture.transport.clone());
        let (panel, visual) = open(&fixture, recording.clone(), &id, cx);
        tap(visual, &format!("ocr-install-{id}"));
        let storage = assert_failure(&recording, &panel, &id, 0, visual);
        assert!(
            storage.message.contains("model asset storage failed"),
            "{storage}"
        );

        std::fs::remove_file(&directory).unwrap();
        std::fs::create_dir(&directory).unwrap();
        for asset in &definition.assets {
            std::fs::write(directory.join(&asset.name), seed(&asset.name)).unwrap();
        }
        tap(visual, "runtime-retry");
        let validation = assert_failure(&recording, &panel, &id, 1, visual);
        assert!(
            validation.message.contains("OCR dictionary is invalid"),
            "{validation}"
        );
        assert_ne!(storage, validation);
        assert_identity(&recording, &id);
        assert!(!directory.join("validated").exists());
        assert!(
            models(&fixture)
                .iter()
                .all(|entry| entry.state != Status::Installed)
        );
        fixture.close(visual);
    }
}

fn seed(name: &str) -> &'static [u8] {
    if name == "dictionary.txt" {
        b"\n"
    } else {
        b"unvalidated graph"
    }
}

#[gpui::test]
#[ignore = "requires the explicit pinned OCR reference fixture and packaged ONNX Runtime"]
fn installs_verified_reference_after_retry(cx: &mut TestAppContext) {
    init(cx);
    let reference = std::env::var_os("SAILRY_OCR_REFERENCE").expect("OCR reference path required");
    let reference = Path::new(&reference).join("pp-ocrv6-tiny");
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let model = models(&fixture)
            .into_iter()
            .find(|model| model.definition.name == "PP-OCRv6 tiny")
            .unwrap();
        let files = model
            .definition
            .assets
            .iter()
            .map(|asset| sailry_model_assets::File {
                name: &asset.name,
                size: asset.bytes,
                sha256: &asset.sha256,
            })
            .collect::<Vec<_>>();
        assert!(
            sailry_model_assets::ready(&reference, &files),
            "pinned OCR reference assets required"
        );
        let directory = blocked_path(&fixture, &model.id);
        let recording = Recording::new(fixture.transport.clone());
        let (panel, visual) = open(&fixture, recording.clone(), &model.id, cx);
        tap(visual, &format!("ocr-install-{}", model.id));
        let storage = assert_failure(&recording, &panel, &model.id, 0, visual);
        assert!(
            storage.message.contains("model asset storage failed"),
            "{storage}"
        );
        std::fs::remove_file(&directory).unwrap();
        std::fs::create_dir(&directory).unwrap();
        for asset in &model.definition.assets {
            std::fs::copy(reference.join(&asset.name), directory.join(&asset.name)).unwrap();
        }
        tap(visual, "runtime-retry");
        wait_for(
            visual,
            |cx| {
                let panel = panel.read(cx);
                !panel.pending
                    && !panel.loading
                    && panel
                        .models
                        .iter()
                        .any(|entry| entry.id == model.id && entry.state == Status::Installed)
            },
            |_, cx| {
                format!(
                    "OCR retry installation deadline: {}",
                    state(&recording, &panel, cx)
                )
            },
        );
        assert_identity(&recording, &model.id);
        visual.update(|_, cx| {
            let panel = panel.read(cx);
            assert!(panel.fault.is_none());
            assert!(panel.request.is_none());
            assert!(panel.retry_install.is_none());
            assert_eq!(panel.binding.client.target(), recording.target());
            assert!(panel.settings.as_ref().unwrap().model.is_none());
        });
        assert!(directory.join("validated").is_file());
        assert_eq!(
            models(&fixture)
                .into_iter()
                .find(|entry| entry.id == model.id)
                .unwrap()
                .state,
            Status::Installed
        );
        fixture.close(visual);
    }
}
