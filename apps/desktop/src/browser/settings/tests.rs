use super::*;
use core::prelude::v1::test;
use gpui_kit::component::{Root, WindowExt as _};

struct Surface;
impl Render for Surface {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

fn metadata() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("Default")).unwrap();
    std::fs::write(root.path().join("Default/Cookies"), b"unopened fixture").unwrap();
    std::fs::write(
        root.path().join("Local State"),
        br#"{"profile":{"info_cache":{"Default":{"name":"Detected"}}}}"#,
    )
    .unwrap();
    root
}

fn mount(cx: &mut TestAppContext) -> (Entity<Settings>, &mut VisualTestContext) {
    cx.update(gpui_kit::init);
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        owner = Some(cx.new(|cx| Settings::new(window, cx)));
        Root::new(cx.new(|_| Surface), window, cx)
    });
    (owner.unwrap(), visual)
}

fn finish(
    owner: &Entity<Settings>,
    visual: &mut VisualTestContext,
    result: super::super::chrome::Result<Vec<super::super::chrome::Profile>>,
    stop: CancellationToken,
) -> Result<Value, String> {
    let (send, mut receive) = tokio::sync::oneshot::channel();
    owner.update(visual, |owner, _| {
        owner.busy = true;
        owner.finish_scan(result, stop, send);
    });
    visual.run_until_parked();
    assert!(!visual.did_prompt_for_paths());
    assert!(!owner.read_with(visual, |owner, _| owner.busy));
    receive.try_recv().unwrap()
}

#[gpui::test]
fn returns_opaque_profiles_without_picker(cx: &mut TestAppContext) {
    let root = metadata();
    let (owner, visual) = mount(cx);
    let bytes = std::fs::read(root.path().join("Local State")).unwrap();
    let profiles = finish(
        &owner,
        visual,
        super::super::chrome::discover(root.path()),
        CancellationToken::new(),
    )
    .unwrap();
    let profiles = profiles.as_array().unwrap();
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0]["name"], "Detected");
    assert_eq!(profiles[0].as_object().unwrap().len(), 2);
    let id = profiles[0]["id"].as_str().unwrap();
    assert_eq!(
        owner.read_with(visual, |owner, _| owner.profiles[id].database.clone()),
        root.path().join("Default/Cookies")
    );
    assert_eq!(
        std::fs::read(root.path().join("Local State")).unwrap(),
        bytes
    );
    assert_eq!(
        std::fs::read(root.path().join("Default/Cookies")).unwrap(),
        b"unopened fixture"
    );
    let replacement = finish(
        &owner,
        visual,
        super::super::chrome::discover(root.path()),
        CancellationToken::new(),
    )
    .unwrap();
    assert_ne!(replacement[0]["id"], id);
    assert!(!owner.read_with(visual, |owner, _| owner.profiles.contains_key(id)));
}

#[gpui::test]
fn denied_and_closed_scans_preserve_retry_choices(cx: &mut TestAppContext) {
    let root = metadata();
    let (owner, visual) = mount(cx);
    let profiles = finish(
        &owner,
        visual,
        super::super::chrome::discover(root.path()),
        CancellationToken::new(),
    )
    .unwrap();
    let id = profiles[0]["id"].as_str().unwrap();
    for cancelled in [false, true] {
        let stop = CancellationToken::new();
        if cancelled {
            stop.cancel();
        }
        let result = if cancelled {
            super::super::chrome::discover(root.path())
        } else {
            Err("browser_chrome_access_denied")
        };
        assert_eq!(
            finish(&owner, visual, result, stop).unwrap_err(),
            if cancelled {
                "plugin view is closed"
            } else {
                "browser_chrome_access_denied"
            }
        );
        assert!(owner.read_with(visual, |owner, _| owner.profiles.contains_key(id)));
    }
}

#[gpui::test]
fn preview_scan_does_not_open_authorization(cx: &mut TestAppContext) {
    let (owner, visual) = mount(cx);
    let (reply, mut receive) = tokio::sync::oneshot::channel();
    owner.update(visual, |_, cx| {
        cx.emit(Request {
            action: Action::Scan,
            stop: CancellationToken::new(),
            reply: RefCell::new(Some(reply)),
        })
    });
    visual.run_until_parked();
    assert_eq!(
        receive.try_recv().unwrap(),
        Err("browser_import_unavailable".into())
    );
    assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
    assert!(visual.opened_url().is_none());
    assert!(owner.read_with(visual, |owner, _| owner.profiles.is_empty() && !owner.busy));
}
