use super::*;
use sailry_protocol::python::{Preparation, Stage};

#[gpui::test]
fn shows_pending_dependencies_before_publication(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        wait(visual, |cx| owner.read(cx).plugin_catalog.python.connected);
        let packages = fixture.public_packages(false);
        for stage in [Stage::Creating, Stage::Installing, Stage::Validating] {
            owner.update(visual, |owner, cx| {
                owner.plugin_catalog.python.preparations = vec![Preparation {
                    package: "pending-fixture".into(),
                    stage,
                }];
                cx.notify();
            });
            shown(visual, "plugin-preparing-pending-fixture", true);
            assert_eq!(fixture.public_packages(false), packages);
        }
        owner.update(visual, |owner, cx| {
            owner.plugin_catalog.python.preparations.clear();
            cx.notify();
        });
        shown(visual, "plugin-preparing-pending-fixture", false);
        owner.update(visual, |owner, cx| {
            owner.plugin_catalog.python.preparations = vec![Preparation {
                package: "pending-fixture".into(),
                stage: Stage::Installing,
            }];
            owner.plugin_catalog.python.connected = false;
            cx.notify();
        });
        shown(visual, "plugin-preparing-pending-fixture", false);
        fixture.close(visual);
    }
}
