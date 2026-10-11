use super::*;
use crate::conversation::live::tests::wait;
use gpui_kit::component::WindowExt;

const VISUAL_FEEDBACK: &str = "Package visual feedback";

pub(in crate::plugins::artifact) struct Draft {
    surface: Entity<View>,
    events: tokio::sync::mpsc::Receiver<Value>,
    revision: u32,
    id: ArtifactId,
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
        };
        draft.feedback(visual);
        draft
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
                visual.update(|window, cx| {
                    window.clear_notifications(cx);
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
                        initial + expected,
                        "each failed attempt must show feedback"
                    );
                    assert_eq!(crate::feedback::tests::summary(window, cx), message);
                    if label.is_some() {
                        assert_eq!(
                            crate::feedback::tests::count(window, &fallback, cx),
                            fallback_count,
                            "the package label must take priority over core feedback"
                        );
                    } else if kind == "render_error" {
                        assert_eq!(
                            crate::feedback::tests::count(window, VISUAL_FEEDBACK, cx),
                            custom_count,
                            "an absent package label must use only core feedback"
                        );
                    }
                    assert_eq!(
                        crate::feedback::tests::count(
                            window,
                            &crate::tr("artifact_preview_failed"),
                            cx
                        ),
                        0,
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
    }
}
