use super::*;
use core::prelude::v1::test;
use gpui_kit::component::{Root, WindowExt as _};
use gpui_kit::test::TestWindowExt as _;

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
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::preferences::init(cx);
        crate::theme::init(cx);
    });
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        owner = Some(cx.new(|cx| Settings::new(window, cx)));
        Root::new(cx.new(|_| Surface), window, cx)
    });
    (owner.unwrap(), visual)
}

fn denied_access(cx: &mut TestAppContext, locale: &str, message: &str) {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let before = rust_i18n::locale().to_string();
    let (_, visual) = mount(cx);
    rust_i18n::set_locale(locale);
    let calls = Arc::new(AtomicUsize::new(0));
    let requests = calls.clone();
    let request: PermissionAction = Rc::new(move |cx, _| {
        let attempt = requests.fetch_add(1, Ordering::SeqCst);
        cx.background_executor().spawn(async move {
            if attempt == 0 {
                Err(access_failure("browser_chrome_access_denied"))
            } else {
                Ok(vec![(Resource::Chrome, Status::Granted)])
            }
        })
    });
    let completed = Rc::new(RefCell::new(Vec::new()));
    let result = completed.clone();
    visual.update(|window, cx| {
        permissions::open(
            vec![
                chrome_card(request),
                Card {
                    resource: Resource::FullDisk,
                    status: Status::NotNeeded,
                    settings: Some(
                        "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles",
                    ),
                    check: None,
                    request: None,
                    requires: None,
                },
            ],
            CancellationToken::new(),
            Box::new(move |granted, _, _| result.borrow_mut().push(granted)),
            window,
            cx,
        );
    });
    visual.run_until_parked();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(visual.opened_url().is_none());
    assert!(!visual.did_prompt_for_paths());
    visual.update(|window, cx| {
        assert_eq!(
            window.find("permissions-continue").label(),
            Some(crate::tr("permission_continue").as_ref())
        );
        window.click("permission_full_disk", cx);
    });
    visual.run_until_parked();
    assert_eq!(
        visual.opened_url().as_deref(),
        Some("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(completed.borrow().is_empty());

    visual.update(|window, cx| window.click("permissions-continue", cx));
    visual.run_until_parked();
    visual.update(|window, cx| {
        assert_eq!(
            crate::feedback::tests::summary(window, cx).as_ref(),
            message
        );
        assert_eq!(crate::feedback::tests::count(window, message, cx), 1);
        assert_eq!(
            window.find("permissions-continue").label(),
            Some(crate::tr("permission_continue").as_ref())
        );
        assert_eq!(
            window.find("permission_full_disk").label(),
            Some(crate::tr("permission_settings").as_ref())
        );
        window.click("permission_full_disk", cx);
    });
    visual.run_until_parked();
    assert_eq!(
        visual.opened_url().as_deref(),
        Some("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(completed.borrow().is_empty());

    visual.update(|window, cx| window.click("permissions-continue", cx));
    visual.run_until_parked();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(&*completed.borrow(), &[true]);
    assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
    visual.update(|window, _| window.remove_window());
    rust_i18n::set_locale(&before);
}

mod access_guide {
    use super::*;

    #[gpui::test]
    fn english(cx: &mut TestAppContext) {
        denied_access(
            cx,
            "en",
            "Chrome data access denied; check Full Disk Access",
        );
    }

    #[gpui::test]
    fn chinese(cx: &mut TestAppContext) {
        denied_access(
            cx,
            "zh-CN",
            "无法访问 Chrome 数据，请检查“完全磁盘访问”设置",
        );
    }
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
