use super::super::*;
use crate::plugins::{
    controls,
    host::sdk::values::{decode, encode},
    layout, overlay,
};
use gpui_shell::{
    HostError, HostModule, HostValue,
    policy::{self, Policy},
};
use sailry_link::CancellationToken;
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};

type Reply = tokio::sync::oneshot::Sender<Result<HostValue, HostError>>;

#[derive(Default)]
struct Calls {
    requests: Vec<String>,
    clicks: Vec<String>,
    pending: VecDeque<(String, Reply)>,
    persistent: Vec<bool>,
    toasts: Vec<Value>,
}

impl Calls {
    fn request(
        &mut self,
        id: String,
    ) -> tokio::sync::oneshot::Receiver<Result<HostValue, HostError>> {
        let (send, receive) = tokio::sync::oneshot::channel();
        self.requests.push(id.clone());
        self.pending.push_back((id, send));
        receive
    }
}

struct DefaultPolicy(Option<Policy>);
impl Drop for DefaultPolicy {
    fn drop(&mut self) {
        policy::set_default(self.0.take().unwrap());
    }
}

struct SettingsFixture {
    calls: Rc<RefCell<Calls>>,
    script: Entity<gpui_shell::ScriptView>,
    _application: Entity<gpui_shell::ShellRoot>,
    _runtime: Rc<gpui_shell::ShellRuntime>,
    _directory: tempfile::TempDir,
    stop: CancellationToken,
}

impl Drop for SettingsFixture {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl SettingsFixture {
    fn complete(&self, id: &str, result: Result<Value, &str>) {
        let (actual, reply) = self.calls.borrow_mut().pending.pop_front().unwrap();
        assert_eq!(actual, id);
        reply
            .send(
                result
                    .map(|value| encode(value).unwrap())
                    .map_err(HostError::new),
            )
            .unwrap();
    }

    fn tree(&self, cx: &App) -> String {
        let script = self.script.read(cx);
        assert_eq!(script.build_error(), None);
        script.snapshot().unwrap().debug_tree()
    }
}

fn mount(cx: &mut TestAppContext) -> (SettingsFixture, &mut VisualTestContext) {
    init(cx);
    let directory = tempfile::tempdir().unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins/browser/dev.sailry.platform/desktop");
    let source = std::fs::read_to_string(root.join("settings.js")).unwrap();
    // Transparent probes and click records preserve the production controls.
    let source = source
        .replace(".child(control));", ".child(Anchor.new(`button-${id}`).child(control)));" )
        .replace(
            "this.profiles.map((profile,index) => new Button(`browser-profile-${index}`)",
            "this.profiles.map((profile,index) => Anchor.new(`profile-${index}`).child(new Button(`browser-profile-${index}`)",
        )
        .replace(
            ".on_click((_,cx) => this.scan(cx))",
            ".on_click((_,cx) => {recordClick('scan');this.scan(cx);})",
        )
        .replace(
            ".on_click((_,cx) => this.import(profile,cx)))))]",
            ".on_click((_,cx) => {recordClick(profile.id);this.import(profile,cx);})))))]",
        )
        .replace(
            "else if (event.id === 'browser-retain-data') this.persist(event.value,cx);",
            "else if (event.id === 'browser-retain-data') {recordClick('retain');this.persist(event.value,cx);}",
        );
    std::fs::write(
        directory.path().join("main.js"),
        format!("import {{Anchor,recordClick}} from 'sailry/test';\n{source}"),
    )
    .unwrap();
    std::fs::copy(root.join("locales.js"), directory.path().join("locales.js")).unwrap();

    let calls = Rc::new(RefCell::new(Calls::default()));
    let stop = CancellationToken::new();
    let retained = Rc::new(Cell::new(true));
    let window = Rc::new(Cell::new(None::<AnyWindowHandle>));
    let ui = cx.update(|cx| {
        controls::module(
            layout::extend(overlay::module(cx)),
            stop.clone(),
            controls::tabs::Controls::new(stop.clone()),
            cx,
        )
    });
    let declarations = format!(
        "{}\n{}",
        ui.declared().unwrap(),
        r#"
        export function readBrowserSettings(): {supported:boolean;enabled:boolean;persistent:boolean};
        export function setBrowserPersistent(value:boolean): void;
        export function listBrowserProfiles(): Promise<{id:string;name:string}[] | null>;
        export function importBrowserProfile(id:string): Promise<{count:number;skipped:number} | null>;
        export function toast(value: {id:string;message:string;kind:string}): void;
        export function selectSettingsHost(): void;
    "#
    );
    let ui = ui
        .function("readBrowserSettings", {
            let retained = retained.clone();
            move |_| encode(json!({"supported":true,"enabled":true,"persistent":retained.get()}))
        })
        .function("setBrowserPersistent", {
            let retained = retained.clone();
            let calls = calls.clone();
            move |args| {
                let value = args.value(0)?.as_bool().unwrap();
                retained.set(value);
                calls.borrow_mut().persistent.push(value);
                Ok(HostValue::Null)
            }
        })
        .async_function("listBrowserProfiles", {
            let calls = calls.clone();
            move |_| {
                let receive = calls.borrow_mut().request("scan".into());
                Ok(async move { receive.await.unwrap() })
            }
        })
        .async_function("importBrowserProfile", {
            let calls = calls.clone();
            move |args| {
                let receive = calls.borrow_mut().request(args.string(0)?.into());
                Ok(async move { receive.await.unwrap() })
            }
        })
        .function("toast", {
            let calls = calls.clone();
            let window = window.clone();
            move |args| {
                let value = decode(args.value(0)?)?;
                let summary = value["message"].as_str().unwrap().to_owned();
                let error = value["kind"] == "error";
                calls.borrow_mut().toasts.push(value);
                let handle = window.get().unwrap();
                gpui_shell::with_current_app(|cx| {
                    cx.defer(move |cx| {
                        handle
                            .update(cx, |_, window, cx| {
                                let message = if error {
                                    notification::Notification::error(summary.clone())
                                } else {
                                    notification::Notification::info(summary.clone())
                                };
                                crate::feedback::toast(window, summary.into(), message, cx);
                            })
                            .unwrap();
                    })
                });
                Ok(HostValue::Null)
            }
        })
        .function("selectSettingsHost", |_| {
            Err(HostError::new(
                "host selection is unavailable in the isolated browser fixture",
            ))
        })
        .declarations(declarations);
    let probes = HostModule::new("sailry/test")
        .component("Anchor", |mut args, _, _| {
            div()
                .debug_selector(|| args.id().to_owned())
                .children(args.take_children())
                .into_any_element()
        })
        .function("recordClick", {
            let calls = calls.clone();
            move |args| {
                calls.borrow_mut().clicks.push(args.string(0)?.into());
                Ok(HostValue::Null)
            }
        });
    let app = HostModule::new("sailry")
        .function("context", |_| Ok(HostValue::from(r#"{"locale":"en"}"#)))
        .function("theme", |_| {
            gpui_shell::with_current_app(|cx| crate::plugins::theme::snapshot(cx))
                .ok_or_else(|| HostError::new("theme requires an active view"))
        })
        .declarations(
            r#"
            export function context(): string;
            export function theme(): {is_dark:boolean;colors:Record<string,string>};
        "#,
        );
    let granted = Policy::new()
        .with_capabilities(
            gpui_shell::Capabilities::new().read_roots([directory.path().to_path_buf()]),
        )
        .with_host_module(app)
        .unwrap()
        .with_host_module(ui)
        .unwrap()
        .with_host_module(probes)
        .unwrap();
    let mut previous = None;
    policy::update_default(|current| {
        previous = Some(current);
        granted
    });
    let _restore = DefaultPolicy(previous);
    let runtime = gpui_component_shell::new_isolated_runtime().unwrap();
    let mut mounted = None;
    let (_, visual) = cx.add_window_view(|native, cx| {
        window.set(Some(native.window_handle()));
        let application = runtime.try_load(directory.path(), native, cx).unwrap();
        let script = application
            .read(cx)
            .content()
            .clone()
            .downcast::<gpui_shell::ScriptView>()
            .unwrap();
        mounted = Some((application, script.clone()));
        Root::new(script, native, cx)
    });
    let (application, script) = mounted.unwrap();
    let fixture = SettingsFixture {
        calls,
        script,
        _application: application,
        _runtime: runtime,
        _directory: directory,
        stop,
    };
    wait(visual, |cx| {
        fixture.tree(cx).contains("browser-settings-page")
    });
    (fixture, visual)
}

fn spinner(fixture: &SettingsFixture, visual: &mut VisualTestContext, anchor: Option<&str>) {
    visual.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
        let tree = fixture.tree(cx);
        let mut ancestors = Vec::new();
        let mut count = 0;
        for line in tree.lines() {
            let depth = line.len() - line.trim_start().len();
            while ancestors
                .last()
                .is_some_and(|(previous, _)| *previous >= depth)
            {
                ancestors.pop();
            }
            if line.trim_start().starts_with("Spinner") {
                count += 1;
                assert!(
                    ancestors
                        .iter()
                        .any(|(_, line): &(usize, &str)| line.trim_start().starts_with("Button")),
                    "{tree}"
                );
                assert!(
                    ancestors
                        .iter()
                        .any(|(_, line)| anchor.is_some_and(|anchor| line.contains(anchor))),
                    "{tree}"
                );
            }
            ancestors.push((depth, line));
        }
        assert_eq!(count, usize::from(anchor.is_some()), "{tree}");
        assert!(!tree.contains("browser-settings-message"));
    });
}

#[gpui::test]
fn imports_single_profile_with_button_loading(cx: &mut TestAppContext) {
    let (fixture, visual) = mount(cx);
    spinner(&fixture, visual, None);
    assert!(fixture.calls.borrow().requests.is_empty());
    click(visual, "button-browser_chrome_import");
    wait(visual, |_| fixture.calls.borrow().requests == ["scan"]);
    spinner(&fixture, visual, Some("button-browser_chrome_import"));
    click(visual, "button-browser_chrome_import");
    click(visual, "browser-retain-data");
    assert_eq!(fixture.calls.borrow().clicks, ["scan"]);
    fixture.complete("scan", Ok(json!([{"id":"detected","name":"Default"}])));
    wait(visual, |_| {
        fixture.calls.borrow().requests == ["scan", "detected"]
    });
    spinner(&fixture, visual, Some("button-browser_chrome_import"));
    assert!(
        !fixture
            .script
            .read_with(visual, |_, cx| fixture.tree(cx))
            .contains("browser-profile-picker")
    );
    click(visual, "button-browser_chrome_import");
    assert_eq!(fixture.calls.borrow().clicks, ["scan"]);
    fixture.complete("detected", Ok(json!({"count":3,"skipped":1})));
    toast(visual, "Imported 3 cookies, skipped 1");
    spinner(&fixture, visual, None);
    assert_eq!(fixture.calls.borrow().toasts.len(), 1);
    assert!(fixture.calls.borrow().persistent.is_empty());
    click(visual, "browser-retain-data");
    wait(visual, |_| fixture.calls.borrow().persistent == [false]);
    visual.update(|window, _| window.remove_window());
}

#[gpui::test]
fn preserves_choices_after_import_failure(cx: &mut TestAppContext) {
    let (fixture, visual) = mount(cx);
    click(visual, "button-browser_chrome_import");
    wait(visual, |_| fixture.calls.borrow().requests == ["scan"]);
    fixture.complete(
        "scan",
        Ok(json!([
            {"id":"first","name":"Personal"},{"id":"second","name":"Work"}
        ])),
    );
    wait(visual, |cx| fixture.tree(cx).contains("profile-1"));
    spinner(&fixture, visual, None);
    assert_eq!(fixture.calls.borrow().requests, ["scan"]);
    click(visual, "profile-1");
    wait(visual, |_| {
        fixture.calls.borrow().requests == ["scan", "second"]
    });
    spinner(&fixture, visual, Some("profile-1"));
    click(visual, "profile-0");
    click(visual, "profile-1");
    assert_eq!(fixture.calls.borrow().clicks, ["scan", "second"]);
    fixture.complete("second", Err("browser_chrome_key_denied"));
    toast(
        visual,
        "Could not read the Chrome key; check Keychain access",
    );
    spinner(&fixture, visual, None);
    assert!(
        fixture
            .script
            .read_with(visual, |_, cx| fixture.tree(cx))
            .contains("profile-1")
    );
    click(visual, "profile-1");
    wait(visual, |_| {
        fixture.calls.borrow().requests == ["scan", "second", "second"]
    });
    fixture.complete("second", Ok(json!({"count":1,"skipped":0})));
    toast(visual, "Imported 1 cookies, skipped 0");
    spinner(&fixture, visual, None);
    assert!(
        !fixture
            .script
            .read_with(visual, |_, cx| fixture.tree(cx))
            .contains("browser-profile-picker")
    );
    assert_eq!(fixture.calls.borrow().clicks, ["scan", "second", "second"]);
    visual.update(|window, _| window.remove_window());
}

#[gpui::test]
fn releases_button_after_scan_ends(cx: &mut TestAppContext) {
    for (result, message) in [
        (
            Err("browser_chrome_access_denied"),
            Some("Chrome data access denied; check Full Disk Access"),
        ),
        (Ok(json!([])), Some("No Chrome profiles found")),
        (Ok(Value::Null), None),
    ] {
        let (fixture, visual) = mount(cx);
        click(visual, "button-browser_chrome_import");
        wait(visual, |_| fixture.calls.borrow().requests == ["scan"]);
        fixture.complete("scan", result);
        wait(visual, |cx| {
            !fixture
                .tree(cx)
                .lines()
                .any(|line| line.trim_start().starts_with("Spinner"))
        });
        if let Some(message) = message {
            toast(visual, message);
        } else {
            assert!(fixture.calls.borrow().toasts.is_empty());
        }
        spinner(&fixture, visual, None);
        assert_eq!(fixture.calls.borrow().requests, ["scan"]);
        assert!(!visual.did_prompt_for_paths());
        assert!(fixture.calls.borrow().persistent.is_empty());
        visual.update(|window, _| window.remove_window());
    }
}

#[gpui::test]
fn preserves_choices_on_permission_cancel(cx: &mut TestAppContext) {
    let (fixture, visual) = mount(cx);
    click(visual, "button-browser_chrome_import");
    wait(visual, |_| fixture.calls.borrow().requests == ["scan"]);
    fixture.complete(
        "scan",
        Ok(json!([
            {"id":"first","name":"Personal"},{"id":"second","name":"Work"}
        ])),
    );
    wait(visual, |cx| fixture.tree(cx).contains("profile-1"));
    click(visual, "profile-1");
    wait(visual, |_| {
        fixture.calls.borrow().requests == ["scan", "second"]
    });
    spinner(&fixture, visual, Some("profile-1"));
    let (id, reply) = fixture.calls.borrow_mut().pending.pop_front().unwrap();
    assert_eq!(id, "second");
    visual.update(|window, cx| {
        crate::permissions::open(
            vec![crate::permissions::Card {
                resource: crate::permissions::Resource::Chrome,
                status: crate::permissions::Status::Unknown,
                settings: None,
                check: None,
                request: None,
                requires: None,
            }],
            CancellationToken::new(),
            Box::new(move |granted, _, _| {
                assert!(!granted);
                reply.send(Ok(HostValue::Null)).unwrap();
            }),
            window,
            cx,
        )
    });
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("permissions-modal").is_some());
    click(visual, "permissions-cancel");
    wait(visual, |cx| !fixture.tree(cx).contains("Spinner"));
    spinner(&fixture, visual, None);
    assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
    fixture.script.read_with(visual, |_, cx| {
        let tree = fixture.tree(cx);
        assert!(tree.contains("profile-0") && tree.contains("profile-1"));
    });
    assert!(fixture.calls.borrow().toasts.is_empty());
    assert!(fixture.calls.borrow().persistent.is_empty());
    click(visual, "profile-1");
    wait(visual, |_| {
        fixture.calls.borrow().requests == ["scan", "second", "second"]
    });
    fixture.complete("second", Ok(json!({"count":1,"skipped":0})));
    toast(visual, "Imported 1 cookies, skipped 0");
    spinner(&fixture, visual, None);
    assert_eq!(fixture.calls.borrow().clicks, ["scan", "second", "second"]);
    visual.update(|window, _| window.remove_window());
}
