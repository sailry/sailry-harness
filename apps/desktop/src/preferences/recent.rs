//! Bounded controller navigation history, independent of project ordering.
use super::*;
use sailry_protocol::{NodeId, SessionId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Visit {
    pub node: NodeId,
    pub session: SessionId,
}

pub(crate) fn record(node: NodeId, session: SessionId, cx: &mut App) {
    let visit = Visit { node, session };
    if cx
        .global::<Preferences>()
        .data
        .recent
        .as_ref()
        .and_then(|visits| visits.first())
        == Some(&visit)
    {
        return;
    }
    super::update(cx, |data| {
        let visits = data.recent.get_or_insert_default();
        visits.retain(|entry| *entry != visit);
        visits.insert(0, visit);
        visits.truncate(100);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{TestAppContext, gpui};

    #[gpui::test]
    fn ordering_and_persistence(cx: &mut TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("preferences.json");
        cx.update(|cx| {
            cx.set_global(Preferences::open(path.clone()));
            let first = Visit {
                node: NodeId([1; 32]),
                session: SessionId::new(),
            };
            let second = Visit {
                node: NodeId([2; 32]),
                session: first.session,
            };
            record(first.node, first.session, cx);
            record(second.node, second.session, cx);
            record(first.node, first.session, cx);
            record(first.node, first.session, cx);
            assert_eq!(
                Preferences::open(path.clone()).data.recent.unwrap(),
                [first, second]
            );
            for _ in 0..105 {
                record(first.node, SessionId::new(), cx);
            }
            assert_eq!(Preferences::open(path).data.recent.unwrap().len(), 100);
        });
    }
}
