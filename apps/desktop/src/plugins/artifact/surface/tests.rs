use super::*;
use crate::conversation::live::tests::wait;

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
        Self {
            surface,
            events: receiver,
            revision,
            id,
        }
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
