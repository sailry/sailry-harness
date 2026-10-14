use super::*;
use crate::conversation::live::tests::wait;
use gpui_kit::component::WindowExt;
use gpui_kit::test::TestWindowExt as _;

const VISUAL_FEEDBACK: &str = "Package visual feedback";

#[derive(Default)]
pub(in crate::plugins::artifact) struct Mounted(
    std::collections::BTreeMap<(sailry_protocol::NodeId, ArtifactId), WeakEntity<View>>,
);

impl Global for Mounted {}

impl Mounted {
    pub(in crate::plugins::artifact) fn owner(
        &self,
        key: (sailry_protocol::NodeId, ArtifactId),
    ) -> Entity<View> {
        self.0.get(&key).and_then(WeakEntity::upgrade).unwrap()
    }
}

fn notification_ids(window: &mut Window, cx: &mut App) -> Vec<EntityId> {
    window
        .notifications(cx)
        .iter()
        .map(Entity::entity_id)
        .collect()
}

#[track_caller]
fn dismissed(visual: &mut VisualTestContext) {
    let interval = std::time::Duration::from_millis(10);
    let timeout = std::time::Duration::from_secs(1);
    let mut elapsed = std::time::Duration::ZERO;
    loop {
        visual.run_until_parked();
        if visual.update(|window, cx| {
            window.render_frame(cx);
            window.notifications(cx).is_empty()
        }) {
            return;
        }
        assert!(elapsed < timeout, "feedback must finish closing");
        visual.executor().advance_clock(interval);
        elapsed += interval;
    }
}

pub(super) fn capture(
    node: sailry_protocol::NodeId,
    artifact: ArtifactId,
    owner: WeakEntity<View>,
    cx: &mut App,
) {
    cx.global_mut::<Mounted>().0.insert((node, artifact), owner);
}

pub(in crate::plugins::artifact) fn mounted_revision(
    node: sailry_protocol::NodeId,
    artifact: ArtifactId,
    cx: &App,
) -> Option<(u32, u32)> {
    let owner = cx.global::<Mounted>().0.get(&(node, artifact))?.upgrade()?;
    let view = owner.read(cx);
    (!view.loading).then_some(())?;
    let content = view.content.as_ref()?;
    Some((content.artifact.revision, content.latest_revision))
}

#[track_caller]
pub(in crate::plugins::artifact) fn settle_mount(
    owner: Entity<View>,
    revision: u32,
    visual: &mut VisualTestContext,
) {
    let preview_failure = crate::tr("artifact_preview_failed");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        visual
            .executor()
            .advance_clock(std::time::Duration::from_millis(10));
        visual.run_until_parked();
        let (state, delivered) = visual.update(|window, cx| {
            window.render_frame(cx);
            let view = owner.read(cx);
            let state = (
                owner.entity_id(),
                view.generation,
                view.loading,
                view.reload,
                view.content
                    .as_ref()
                    .map(|content| (content.artifact.revision, content.latest_revision)),
                view.error,
                view.native.is_some(),
            );
            (
                state,
                crate::feedback::tests::count(window, &preview_failure, cx),
            )
        });
        // The package header can settle before its independently loaded body.
        // TestPlatform cannot mount WebKit; observe that owner's initial feedback
        // before the detached draft starts measuring operation feedback.
        if !state.2
            && !state.3
            && state.4 == Some((revision, revision))
            && state.5 == Some("artifact_preview_failed")
            && !state.6
            && delivered > 0
        {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "mounted artifact feedback deadline: state={state:?}, delivered={delivered}"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

pub(in crate::plugins::artifact) struct Draft {
    surface: Entity<View>,
    events: tokio::sync::mpsc::Receiver<Value>,
    revision: u32,
    id: ArtifactId,
    cache: tempfile::TempDir,
}

impl Draft {
    pub(in crate::plugins::artifact) fn new(
        resource: Resource,
        host: Arc<Host>,
        visual: &mut VisualTestContext,
    ) -> Self {
        let id = resource.artifact.id;
        let session = resource.artifact.session;
        let revision = resource.artifact.revision;
        let cache = tempfile::tempdir().unwrap();
        visual.update(|_, cx| {
            cx.set_global(crate::preferences::Preferences::open(
                cache.path().join("preferences.json"),
            ));
        });
        let (events, receiver) = tokio::sync::mpsc::channel(4);
        let surface = visual.update(|window, cx| {
            cx.new(|cx| {
                View::new(
                    resource,
                    Props {
                        artifact: id,
                        revision: None,
                        labels: [("render_error".into(), VISUAL_FEEDBACK.into())]
                            .into_iter()
                            .collect(),
                    },
                    host,
                    events,
                    window,
                    cx,
                )
            })
        });
        wait(visual, |cx| {
            let view = surface.read(cx);
            !view.loading
                && view.content.as_ref().is_some_and(|content| {
                    content.artifact.revision == revision && content.latest_revision == revision
                })
        });
        visual.update(|window, cx| {
            surface.update(cx, |view, cx| {
                assert_eq!(view.revision, None);
                view.event(
                    json!({"kind":"edited","artifact":id,"session":session,"revision":revision})
                        .to_string(),
                    window,
                    cx,
                );
                assert!(view.dirty);
            });
        });
        let draft = Self {
            surface,
            events: receiver,
            revision,
            id,
            cache,
        };
        draft.persist(visual);
        draft.feedback(visual);
        draft
    }

    fn persist(&self, visual: &mut VisualTestContext) {
        visual.update(|window, cx| self.surface.update(cx, |view, cx| {
            let content = view.content.as_ref().unwrap();
            view.event(json!({"kind":"draft","artifact":self.id,"session":content.artifact.session,
                "revision":self.revision,"form":"choices","fields":[{"name":"storage","values":["node"]}]}).to_string(),window,cx);
        }));
    }

    pub(in crate::plugins::artifact) fn answer(&self, visual: &mut VisualTestContext) {
        visual.update(|window, cx| self.surface.update(cx, |view, cx| {
            let content = view.content.as_ref().unwrap();
            view.event(json!({"kind":"submit","artifact":self.id,"session":content.artifact.session,
                "revision":self.revision,"form":"choices","fields":[{"name":"storage","label":"Storage",
                "values":[{"value":"node","label":"Execution Node"}]}]}).to_string(),window,cx);
        }));
    }

    pub(in crate::plugins::artifact) fn close(self) -> tempfile::TempDir {
        drop(self.surface);
        self.cache
    }

    fn feedback(&self, visual: &mut VisualTestContext) {
        let content = self
            .surface
            .read_with(visual, |view, _| view.content.clone().unwrap());
        let fallback = crate::tr("artifact_render_failed");
        assert_ne!(fallback.as_ref(), "artifact_render_failed");
        assert_ne!(fallback.as_ref(), VISUAL_FEEDBACK);
        let fallback_count =
            visual.update(|window, cx| crate::feedback::tests::count(window, &fallback, cx));
        let preview_failure_count = visual.update(|window, cx| {
            crate::feedback::tests::count(window, &crate::tr("artifact_preview_failed"), cx)
        });
        for (kind, message, label) in [
            ("required", crate::tr("artifact_form_required"), None),
            ("invalid_form", crate::tr("artifact_form_invalid"), None),
            ("submit", crate::tr("artifact_form_invalid"), None),
            (
                "render_error",
                VISUAL_FEEDBACK.into(),
                Some(VISUAL_FEEDBACK),
            ),
            ("render_error", fallback.clone(), None),
        ] {
            let initial =
                visual.update(|window, cx| crate::feedback::tests::count(window, &message, cx));
            let custom_count = visual
                .update(|window, cx| crate::feedback::tests::count(window, VISUAL_FEEDBACK, cx));
            for expected in 1..=2 {
                let (before_clear, after_clear) = visual.update(|window, cx| {
                    let before = notification_ids(window, cx);
                    window.clear_notifications(cx);
                    (before, notification_ids(window, cx))
                });
                dismissed(visual);
                let after_helper = visual.update(notification_ids);
                visual.update(|window, cx| {
                    let before_event = notification_ids(window, cx);
                    assert!(
                        before_event.is_empty(),
                        "previous feedback must finish closing: kind={kind}, expected={expected}, \
                         notifications={before_clear:?}->{after_clear:?}->{after_helper:?}->{before_event:?}, \
                         message_count={}, custom_count={}",
                        crate::feedback::tests::count(window, &message, cx),
                        crate::feedback::tests::count(window, VISUAL_FEEDBACK, cx),
                    );
                    self.surface.update(cx, |view, cx| {
                        if kind == "render_error" {
                            view.labels.remove("render_error");
                            if let Some(label) = label {
                                view.labels.insert("render_error".into(), label.into());
                            }
                        }
                        view.event(
                            json!({"kind":kind,"artifact":self.id,
                                "session":content.artifact.session,"revision":self.revision})
                            .to_string(),
                            window,
                            cx,
                        );
                        assert!(view.dirty, "feedback must retain the draft");
                        assert_eq!(view.content.as_ref(), Some(&content));
                        assert_eq!(view.error, None, "operation errors are transient");
                    });
                });
                visual.run_until_parked();
                visual.update(|window, cx| {
                    assert_eq!(
                        crate::feedback::tests::count(window, &message, cx),
                        initial + if kind == "render_error" { 0 } else { expected },
                        "only operation failures show feedback"
                    );
                    if kind == "render_error" {
                        assert!(window.notifications(cx).is_empty());
                        assert_eq!(
                            crate::feedback::tests::count(window, &fallback, cx),
                            fallback_count,
                            "visual fallback must not publish a core toast"
                        );
                        assert_eq!(
                            crate::feedback::tests::count(window, VISUAL_FEEDBACK, cx),
                            custom_count,
                            "captured package labels must not publish a visual toast"
                        );
                    } else {
                        assert_eq!(crate::feedback::tests::summary(window, cx), message);
                        assert_eq!(
                            crate::feedback::tests::count(window, &fallback, cx),
                            fallback_count,
                            "operation feedback must not publish a visual fallback"
                        );
                    }
                    assert_eq!(
                        crate::feedback::tests::count(
                            window,
                            &crate::tr("artifact_preview_failed"),
                            cx
                        ),
                        preview_failure_count,
                        "a failed visual must not report a whole-page failure"
                    );
                });
            }
        }
        self.surface.update(visual, |view, _| {
            view.labels
                .insert("render_error".into(), VISUAL_FEEDBACK.into());
        });
        let required = crate::tr("artifact_form_required");
        let initial =
            visual.update(|window, cx| crate::feedback::tests::count(window, &required, cx));
        visual.update(|window, cx| {
            window.clear_notifications(cx);
            self.surface.update(cx, |view, cx| {
                for (artifact, session, revision) in [
                    (ArtifactId::new(), content.artifact.session, self.revision),
                    (self.id, sailry_protocol::SessionId::new(), self.revision),
                    (self.id, content.artifact.session, self.revision + 1),
                ] {
                    view.event(
                        json!({"kind":"required","artifact":artifact,
                            "session":session,"revision":revision})
                        .to_string(),
                        window,
                        cx,
                    );
                }
                assert!(view.dirty);
                assert_eq!(view.content.as_ref(), Some(&content));
                assert_eq!(view.error, None);
            });
        });
        visual.run_until_parked();
        visual.update(|window, cx| {
            assert_eq!(
                crate::feedback::tests::count(window, &required, cx),
                initial,
                "events from another artifact, session or revision must not show feedback"
            );
        });
        visual.update(|window, cx| {
            self.surface.update(cx, |view, cx| {
                view.content.as_mut().unwrap().view = None;
                view.mount(window, cx);
                assert_eq!(view.error, Some("artifact_preview_failed"));
                view.content = Some(content.clone());
                cx.notify();
            });
        });
        crate::feedback::tests::shown(visual);
        visual.update(|window, cx| {
            assert_eq!(
                crate::feedback::tests::summary(window, cx),
                crate::tr("artifact_preview_failed")
            );
            self.surface.update(cx, |view, cx| {
                view.error = None;
                cx.notify();
            });
        });
        visual.run_until_parked();
    }

    pub(in crate::plugins::artifact) fn verify(
        mut self,
        latest: u32,
        visual: &mut VisualTestContext,
    ) {
        wait(visual, |cx| {
            let view = self.surface.read(cx);
            !view.loading
                && view.dirty
                && view.revision == Some(self.revision)
                && view.content.as_ref().is_some_and(|content| {
                    content.artifact.revision == self.revision && content.latest_revision == latest
                })
        });
        visual.update(|window, cx| {
            self.surface.update(cx, |view, cx| {
                let generation = view.generation;
                let content = view.content.clone().unwrap();
                // The package has not consumed its edit event and still renders follow mode.
                view.select(None, window, cx);
                assert_eq!(view.revision, Some(self.revision));
                assert!(view.dirty);
                assert!(!view.loading);
                assert_eq!(view.generation, generation);
                assert_eq!(view.content.as_ref(), Some(&content));
            });
        });
        let stale = crate::tr("artifact_form_stale");
        assert_ne!(stale.as_ref(), "artifact_form_stale");
        let failure = crate::tr("chat_action_failed");
        let (stale_count, failure_count) = visual.update(|window, cx| {
            (
                crate::feedback::tests::count(window, &stale, cx),
                crate::feedback::tests::count(window, &failure, cx),
            )
        });
        for expected in 1..=2 {
            visual.update(|window, cx| {
                window.clear_notifications(cx);
                self.surface.update(cx, |view, cx| {
                    let content = view.content.clone().unwrap();
                    assert!(content.latest_revision > content.artifact.revision);
                    let generation = view.generation;
                    view.event(
                        json!({"kind":"submit","artifact":self.id,
                            "session":content.artifact.session,"revision":self.revision,
                            "form":"decisions","fields":[{"name":"storage",
                                "label":"Storage","values":[{"value":"node","label":"Node"}]}]})
                        .to_string(),
                        window,
                        cx,
                    );
                    assert!(view.dirty, "a stale submission must retain the draft");
                    assert_eq!(view.revision, Some(self.revision));
                    assert_eq!(view.content.as_ref(), Some(&content));
                    assert_eq!(view.generation, generation);
                    assert_eq!(view.error, None, "a stale submission is not a page failure");
                });
            });
            visual.run_until_parked();
            visual.update(|window, cx| {
                assert_eq!(
                    crate::feedback::tests::count(window, &stale, cx),
                    stale_count + expected,
                    "each stale submission must show feedback"
                );
                assert_eq!(crate::feedback::tests::summary(window, cx), stale);
                assert_eq!(
                    crate::feedback::tests::count(window, &failure, cx),
                    failure_count,
                    "a stale submission must not report a generic action failure"
                );
            });
        }
        assert_eq!(
            self.events.try_recv().unwrap(),
            json!({"kind":"edited","artifact":self.id,"revision":self.revision})
        );
        visual.update(|window, cx| {
            self.surface
                .update(cx, |view, cx| view.select(Some(latest), window, cx));
        });
        wait(visual, |cx| {
            let view = self.surface.read(cx);
            !view.dirty
                && !view.loading
                && view.revision == Some(latest)
                && view
                    .content
                    .as_ref()
                    .is_some_and(|content| content.artifact.revision == latest)
        });
        let (resource, host) = self
            .surface
            .read_with(visual, |view, _| (view.resource.clone(), view.host.clone()));
        let node = resource.binding.client.target();
        let path = self.cache.path().join("preferences.json");
        let id = self.id;
        let revision = self.revision;
        let cache = self.close();
        visual.update(|_, cx| {
            cx.set_global(crate::preferences::Preferences::open(path));
            assert_eq!(
                serde_json::to_value(crate::preferences::artifacts::drafts(
                    node, id, revision, cx
                ))
                .unwrap(),
                json!([{"form":"choices","fields":[{"name":"storage","values":["node"]}]}])
            );
            assert!(crate::preferences::artifacts::drafts(node, id, latest, cx).is_empty());
        });
        let (events, mut restored) = tokio::sync::mpsc::channel(4);
        let reopened = visual.update(|window, cx| {
            cx.new(|cx| {
                View::new(
                    resource,
                    Props {
                        artifact: id,
                        revision: None,
                        labels: Default::default(),
                    },
                    host,
                    events,
                    window,
                    cx,
                )
            })
        });
        wait(visual, |cx| {
            let view = reopened.read(cx);
            view.dirty
                && !view.loading
                && view.revision == Some(revision)
                && view.content.as_ref().is_some_and(|content| {
                    content.artifact.revision == revision && content.latest_revision == latest
                })
        });
        assert_eq!(
            restored.try_recv().unwrap(),
            json!({"kind":"edited","artifact":id,"revision":revision})
        );
        drop(reopened);
        drop(cache);
    }
}
