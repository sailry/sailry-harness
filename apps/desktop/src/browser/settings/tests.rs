use super::*;
use core::prelude::v1::test;
use gpui_kit::component::Root;
use std::time::{Duration, Instant};

struct Surface;
impl Render for Surface {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

fn reply(
    visual: &mut VisualTestContext,
    mut receive: tokio::sync::oneshot::Receiver<Result<Value, String>>,
) -> Result<Value, String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        visual.run_until_parked();
        match receive.try_recv() {
            Ok(result) => return result,
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
            Err(error) => panic!("scan reply closed: {error}"),
        }
        assert!(Instant::now() < deadline, "scan reply deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
fn permission_selection_cancel_and_closed_view(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("Default")).unwrap();
    std::fs::write(root.path().join("Default/Cookies"), b"unopened fixture").unwrap();
    std::fs::write(
        root.path().join("Local State"),
        br#"{"profile":{"info_cache":{"Default":{"name":"Selected"}}}}"#,
    )
    .unwrap();
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        owner = Some(cx.new(|cx| Settings::new(window, cx)));
        Root::new(cx.new(|_| Surface), window, cx)
    });
    let owner = owner.unwrap();
    for case in 0..3 {
        let stop = CancellationToken::new();
        let (send, receive) = tokio::sync::oneshot::channel();
        visual.update(|_, cx| {
            owner.update(cx, |owner, cx| {
                owner.finish_scan(Err("browser_chrome_access_denied"), stop.clone(), send, cx)
            })
        });
        visual.run_until_parked();
        assert!(visual.did_prompt_for_paths());
        assert!(owner.read_with(visual, |owner, _| owner.busy));
        if case == 2 {
            stop.cancel();
        }
        visual.simulate_path_prompt_response(|options| {
            assert!(!options.files && options.directories && !options.multiple);
            (case != 1).then(|| vec![root.path().to_path_buf()])
        });
        let result = reply(visual, receive);
        match case {
            0 => {
                let profiles = result.unwrap();
                let profiles = profiles.as_array().unwrap();
                assert_eq!(profiles.len(), 1);
                assert_eq!(profiles[0]["name"], "Selected");
                assert_eq!(profiles[0].as_object().unwrap().len(), 2);
                let id = profiles[0]["id"].as_str().unwrap();
                assert_eq!(
                    owner.read_with(visual, |owner, _| owner.profiles[id].database.clone()),
                    root.path().join("Default/Cookies")
                );
            }
            1 => assert_eq!(result.unwrap(), Value::Null),
            _ => assert_eq!(result.unwrap_err(), "plugin view is closed"),
        }
        assert!(!owner.read_with(visual, |owner, _| owner.busy));
    }
}
