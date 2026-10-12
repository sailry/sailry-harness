use super::*;
use core::prelude::v1::test;
use gpui_kit::test::TestWindowExt as _;
use sailry_client::Client;
use sailry_link::{Admission, Pending, Subscription, Transport};
use sailry_protocol::{Fault, Request, Topic};
use std::sync::{Arc, Mutex};

struct Delayed {
    inner: Arc<dyn Transport>,
    held: Mutex<Vec<Held>>,
}

struct Held {
    request: Request,
    result: Option<Result<Output, Fault>>,
    sender: Option<tokio::sync::oneshot::Sender<Result<Output, Fault>>>,
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
            let read = matches!(
                request.command,
                Command::SearchModels(_)
                    | Command::ReadModelVersions { .. }
                    | Command::ResolveModel { .. }
            );
            let admission = self.inner.dispatch(request.clone()).await?;
            if !read {
                return Ok(admission);
            }
            assert!(!admission.receipt.durable);
            // Hold the reply after admission; cancelling a wait does not cancel Node work.
            let result = admission
                .completion
                .await
                .expect("catalog completion expected");
            let (sender, completion) = tokio::sync::oneshot::channel();
            self.held.lock().unwrap().push(Held {
                request,
                result: Some(result),
                sender: Some(sender),
            });
            Ok(Admission {
                receipt: admission.receipt,
                completion,
            })
        })
    }
}

impl Delayed {
    fn closed(&self, index: usize) -> bool {
        self.held.lock().unwrap().get(index).is_some_and(|held| {
            held.sender
                .as_ref()
                .is_some_and(|sender| sender.is_closed())
        })
    }

    fn complete(&self, index: usize) -> bool {
        let mut held = self.held.lock().unwrap();
        let held = &mut held[index];
        held.sender
            .take()
            .unwrap()
            .send(held.result.take().unwrap())
            .is_ok()
    }
}

fn delay(
    fixture: &Fixture,
    library: &Entity<Library>,
    visual: &mut VisualTestContext,
) -> Arc<Delayed> {
    let delayed = Arc::new(Delayed {
        inner: fixture.transport.clone(),
        held: Mutex::default(),
    });
    visual.update(|_, cx| {
        library.update(cx, |library, _| {
            library.binding.client = Arc::new(Client::new(delayed.clone()));
        });
    });
    delayed
}

#[gpui::test]
fn first_search_waits_before_empty(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        for failed in [false, true] {
            let fixture = Fixture::new(remote);
            *fixture.transport.catalog.lock().unwrap() = Some(definition());
            let (owner, visual) = fixture.mount(cx);
            let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
            let delayed = Arc::new(Delayed {
                inner: fixture.transport.clone(),
                held: Mutex::default(),
            });
            let library = visual.update(|window, cx| {
                let mut binding = owner
                    .read(cx)
                    .provider_link
                    .as_ref()
                    .unwrap()
                    .binding
                    .clone();
                binding.client = Arc::new(Client::new(delayed.clone()));
                open(Target::Knowledge(panel), binding, window, cx)
            });
            wait(visual, |_| delayed.held.lock().unwrap().len() == 1);
            assert!(visual.debug_bounds("model-library-loading").is_some());
            assert!(library.read_with(visual, |library, _| {
                library.first_read() && !library.header_loading()
            }));
            assert!(visual.debug_bounds("empty-model_library_empty").is_none());
            assert!(visual.debug_bounds("model-library-retry").is_none());
            delayed.held.lock().unwrap()[0].result = Some(if failed {
                Err(Fault::invalid("injected catalog failure"))
            } else {
                Ok(Output::ModelSearch(sailry_protocol::models::Page {
                    items: Vec::new(),
                    next_cursor: None,
                    partial: false,
                }))
            });
            assert!(delayed.complete(0));
            wait(visual, |cx| !library.read(cx).loading);
            assert!(visual.debug_bounds("model-library-loading").is_none());
            assert_eq!(
                visual.debug_bounds("empty-model_library_empty").is_some(),
                !failed
            );
            assert_eq!(visual.debug_bounds("model-library-retry").is_some(), failed);
            fixture.close(visual);
        }
    }
}

#[gpui::test]
fn cancels_search_when_catalog_closes(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        *fixture.transport.catalog.lock().unwrap() = Some(definition());
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        tap(visual, "knowledge-model-library");
        let library =
            visual.update(|_, cx| panel.read(cx).library.as_ref().unwrap().upgrade().unwrap());
        wait(visual, |cx| {
            !library.read(cx).loading && library.read(cx).items.len() == 1
        });
        let delayed = delay(&fixture, &library, visual);
        let mut late = definition();
        late.name = "Late search result".into();
        *fixture.transport.catalog.lock().unwrap() = Some(late);
        tap(visual, "model-library-query");
        visual.update(|window, cx| {
            window.press("secondary-a", cx);
            window.input("encoder", cx);
        });
        draw(visual);
        let before = visual.debug_bounds("model-library-result-0").unwrap();
        tap(visual, "model-library-search");
        wait(visual, |cx| {
            library.read(cx).loading && delayed.held.lock().unwrap().len() == 1
        });
        assert!(matches!(
            &delayed.held.lock().unwrap()[0].request.command,
            Command::SearchModels(_)
        ));
        assert_eq!(visual.debug_bounds("model-library-result-0"), Some(before));
        assert!(visual.debug_bounds("model-library-loading").is_none());
        assert!(library.read_with(visual, |library, _| {
            !library.first_read() && library.header_loading()
        }));
        visual.update(|window, cx| window.press("escape", cx));
        draw(visual);
        wait(visual, |_| delayed.closed(0));
        assert!(
            !delayed.complete(0),
            "closed catalog must release its read completion"
        );
        visual.update(|window, cx| {
            assert!(library.read(cx).closed);
            assert!(!library.read(cx).loading);
            assert!(library.read(cx).read.is_none());
            assert_eq!(library.read(cx).items[0].name, definition().name);
            library.update(cx, |library, cx| library.search(false, window, cx));
        });
        assert_eq!(delayed.held.lock().unwrap().len(), 1);
        fixture.close(visual);
    }
}

fn close_details(visual: &mut VisualTestContext) {
    std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
    draw(visual);
    visual.update(|window, cx| {
        let mut dialog = window.within("dialog-1");
        let mut close = dialog.within("gpui_component::dialog::footer::DialogClose");
        assert_ne!(close.find("close").disabled(), Some(true));
        close.click("close", cx);
    });
    draw(visual);
}

#[gpui::test]
fn cancels_versions_and_resolution_on_close(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        *fixture.transport.catalog.lock().unwrap() = Some(definition());
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        tap(visual, "knowledge-model-library");
        let library =
            visual.update(|_, cx| panel.read(cx).library.as_ref().unwrap().upgrade().unwrap());
        wait(visual, |cx| {
            !library.read(cx).loading && library.read(cx).items.len() == 1
        });
        let delayed = delay(&fixture, &library, visual);
        tap(visual, "model-library-details-0");
        wait(visual, |_| delayed.held.lock().unwrap().len() == 1);
        assert!(matches!(
            &delayed.held.lock().unwrap()[0].request.command,
            Command::ReadModelVersions { .. }
        ));
        close_details(visual);
        wait(visual, |_| delayed.closed(0));
        assert!(!delayed.complete(0));
        visual.update(|_, cx| {
            assert!(!library.read(cx).loading);
            assert!(library.read(cx).repository.is_none());
            assert!(library.read(cx).definition.is_none());
        });
        tap(visual, "model-library-details-0");
        wait(visual, |_| delayed.held.lock().unwrap().len() == 2);
        assert!(delayed.complete(1));
        wait(visual, |_| delayed.held.lock().unwrap().len() == 3);
        assert!(matches!(
            &delayed.held.lock().unwrap()[2].request.command,
            Command::ResolveModel { .. }
        ));
        close_details(visual);
        wait(visual, |_| delayed.closed(2));
        assert!(!delayed.complete(2));
        visual.update(|_, cx| {
            assert!(!library.read(cx).loading);
            assert!(library.read(cx).repository.is_none());
            assert!(library.read(cx).definition.is_none());
        });
        assert!(
            fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .all(|request| !matches!(request.command, Command::RegisterKnowledgeModel { .. }))
        );
        fixture.close(visual);
    }
}

#[gpui::test]
fn cancels_resolution_after_target_change(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        *fixture.transport.catalog.lock().unwrap() = Some(definition());
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        tap(visual, "knowledge-model-library");
        let library =
            visual.update(|_, cx| panel.read(cx).library.as_ref().unwrap().upgrade().unwrap());
        wait(visual, |cx| {
            !library.read(cx).loading && library.read(cx).items.len() == 1
        });
        let delayed = delay(&fixture, &library, visual);
        tap(visual, "model-library-install-0");
        wait(visual, |_| delayed.held.lock().unwrap().len() == 1);
        assert!(matches!(
            &delayed.held.lock().unwrap()[0].request.command,
            Command::ResolveModel { .. }
        ));
        fixture.bind(&owner, visual, true);
        wait(visual, |_| delayed.closed(0));
        assert!(!delayed.complete(0));
        visual.update(|_, cx| {
            assert!(!library.read(cx).available(cx));
            assert!(!library.read(cx).loading);
            assert!(library.read(cx).read.is_none());
        });
        assert!(
            fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .all(|request| !matches!(request.command, Command::RegisterKnowledgeModel { .. }))
        );
        fixture.close(visual);
    }
}

#[gpui::test]
fn replaces_reads_when_source_changes(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        *fixture.transport.catalog.lock().unwrap() = Some(definition());
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        tap(visual, "knowledge-model-library");
        let library =
            visual.update(|_, cx| panel.read(cx).library.as_ref().unwrap().upgrade().unwrap());
        wait(visual, |cx| {
            !library.read(cx).loading && library.read(cx).items.len() == 1
        });
        let delayed = delay(&fixture, &library, visual);
        tap(visual, "model-library-search");
        wait(visual, |_| delayed.held.lock().unwrap().len() == 1);
        let mut next = definition();
        next.name = "New source result".into();
        next.source.hub = Hub::ModelScope;
        *fixture.transport.catalog.lock().unwrap() = Some(next.clone());
        visual.update(|window, cx| {
            let hub = library.read(cx).hub.clone();
            hub.update(cx, |hub, cx| {
                hub.set_selected_value(&Hub::ModelScope, window, cx);
                cx.emit(SelectEvent::Confirm(hub.selected_value().cloned()));
            });
        });
        wait(visual, |_| {
            delayed.closed(0) && delayed.held.lock().unwrap().len() == 2
        });
        assert!(visual.debug_bounds("model-library-loading").is_some());
        assert!(visual.debug_bounds("model-library-result-0").is_none());
        assert!(library.read_with(visual, |library, _| {
            library.first_read() && !library.header_loading()
        }));
        assert!(visual.debug_bounds("empty-model_library_empty").is_none());
        assert!(!delayed.complete(0));
        assert!(matches!(&delayed.held.lock().unwrap()[1].request.command,
            Command::SearchModels(search) if search.hub == Hub::ModelScope));
        assert!(delayed.complete(1));
        wait(visual, |cx| {
            !library.read(cx).loading && library.read(cx).items.len() == 1
        });
        visual.update(|_, cx| {
            assert_eq!(library.read(cx).hub(cx), Hub::ModelScope);
            assert_eq!(library.read(cx).items[0].name, next.name);
            assert_eq!(library.read(cx).items[0].source, next.source);
        });
        fixture.close(visual);
    }
}

#[gpui::test]
fn cancels_reads_with_released_owner(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        *fixture.transport.catalog.lock().unwrap() = Some(definition());
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        tap(visual, "knowledge-model-library");
        let library =
            visual.update(|_, cx| panel.read(cx).library.as_ref().unwrap().upgrade().unwrap());
        wait(visual, |cx| {
            !library.read(cx).loading && library.read(cx).items.len() == 1
        });
        let delayed = delay(&fixture, &library, visual);
        tap(visual, "model-library-search");
        wait(visual, |_| delayed.held.lock().unwrap().len() == 1);
        let released = library.downgrade();
        let runtime = fixture.runtime.clone();
        drop(library);
        drop(panel);
        drop(owner);
        fixture.close(visual);
        cx.run_until_parked();
        assert!(released.upgrade().is_none());
        let mut held = delayed.held.lock().unwrap().pop().unwrap();
        let mut sender = held.sender.take().unwrap();
        runtime.block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(10), sender.closed())
                .await
                .expect("released owner must drop its read completion");
        });
        assert!(sender.send(held.result.take().unwrap()).is_err());
    }
}
