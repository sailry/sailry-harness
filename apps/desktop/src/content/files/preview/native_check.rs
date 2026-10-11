//! Native WebKit acceptance uses an isolated HTML fixture, never user files or screenshots.
use super::*;
use gpui_kit::prelude::FluentBuilder;
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType, NSView};
use objc2_foundation::NSPoint;
use std::{path::Path, time::Duration};
use wry::WebViewExtMacOS;

const HTML: &str = r#"<!doctype html><html><body><button id="run" onclick="this.textContent='Done';parent.postMessage('fixture-clicked','*')">Run</button><div style="height:2000px">Scrollable document</div><script>
addEventListener('message', event => {
 if(event.data==='fixture-click')document.querySelector('#run').click();
 if(event.data==='fixture-scroll'){scrollTo(0,300);parent.postMessage('fixture-scrolled','*');}
});
setInterval(()=>parent.postMessage('fixture-ready','*'),100);
</script></body></html>"#;

#[path = "documents/office_fixtures.rs"]
mod office_fixtures;

pub(super) fn prepare(builder: wry::WebViewBuilder<'_>) -> wry::WebViewBuilder<'_> {
    if std::env::var("SAILRY_WORKLOAD_CASE").as_deref() != Ok("file-preview") {
        return builder;
    }
    // A locked macOS session suspends WebKit's animation frames. Drive only
    // this isolated workload's clock; production keeps normal power saving.
    // The real PDF renderer, DOM, native visibility and pointer checks remain.
    builder
        .with_background_throttling(wry::BackgroundThrottlingPolicy::Disabled)
        .with_initialization_script(
            "window.requestAnimationFrame=callback=>setTimeout(()=>callback(performance.now()),16);window.cancelAnimationFrame=id=>clearTimeout(id);",
        )
}

struct Harness {
    native: Option<Rc<surface::Native>>,
    mounted: bool,
    crop: bool,
    hovered: Rc<Cell<bool>>,
    trigger: Rc<Cell<Bounds<Pixels>>>,
    overlay: Option<Pixels>,
    height: Pixels,
    width: Pixels,
    viewport: Pixels,
}
impl Render for Harness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let hovered = self.hovered.clone();
        let trigger = self.trigger.clone();
        v_flex()
            .size_full()
            .child(
                div()
                    .relative()
                    .w(self.width)
                    .h(self.viewport)
                    .overflow_hidden()
                    .when(self.mounted, |region| {
                        region.child(
                            div()
                                .absolute()
                                .top(if self.crop { px(-40.) } else { px(0.) })
                                .w(self.width)
                                .h(self.height)
                                .child(surface::render(
                                    self.native.as_ref().unwrap().clone(),
                                    window,
                                    cx,
                                )),
                        )
                    }),
            )
            .when_some(self.overlay, |this, left| {
                this.child(deferred(
                    div()
                        .id("preview-overlay")
                        .absolute()
                        .left(left)
                        .top_0()
                        .w(px(120.))
                        .h(px(60.))
                        .occlude()
                        .bg(cx.theme().popover)
                        .child("Preview menu"),
                ))
            })
            .child(
                div()
                    .id("hover-region")
                    .mt_8()
                    .w(px(120.))
                    .h(px(40.))
                    .on_prepaint(move |bounds, _, _| trigger.set(bounds))
                    .on_hover(move |value, _, _| hovered.set(*value))
                    .child(
                        Button::new("send-fixture")
                            .label("Send")
                            .tooltip("Send fixture"),
                    ),
            )
    }
}

pub(in crate::content::files) fn run(output: &Path) {
    let output = output.to_owned();
    let documents = std::cell::RefCell::new(Some(office_fixtures::documents(&output)));
    gpui_kit::application().with_assets(crate::assets::Assets).run(move |cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        cx.activate(true);
        let documents = documents.borrow_mut().take().unwrap();
        cx.spawn(async move |cx| {
            let _ = crate::workload::check(async {
            let (finished, completion) = tokio::sync::oneshot::channel();
            cx.open_window(WindowOptions::default(), move |window, cx| {
                window.set_window_title("Sailry — file preview fixture");
                window.activate_window();
                let native = Rc::new(surface::Native::new(HTML, window).unwrap());
                let view = cx.new(|_| Harness { native: Some(native), mounted: true, crop: true, hovered: Rc::default(), trigger: Rc::default(), overlay: None, height: px(180.), width: px(300.), viewport: px(100.) });
                let probe = view.clone();
                window.spawn(cx, async move |cx| {
                    let _ = crate::workload::check(async {
                    verify(&probe, cx).await;
                    verify_documents(&probe, documents, cx).await;
                    verify_unattached(&probe, cx);
                    std::fs::write(output.join("file-preview.json"), "{\"controlled_frames\":true,\"webkit\":true,\"javascript\":true,\"host_hover\":true,\"clipping\":true,\"overlays\":true,\"unmount\":true,\"unattached\":true,\"pdf\":true,\"docx\":true,\"xlsx\":true,\"pptx\":true,\"pagination\":true,\"zoom\":true}\n").unwrap();
                    println!("File preview: HTML, PDF, DOCX, XLSX, PPTX, pagination, zoom, host hover, clipping, overlays, unmount and unattached failure verified");
                    probe.update(cx, |view, cx| { view.mounted = false; view.native.take(); cx.notify(); });
                    pause(cx).await;
                    cx.update(|window, cx| { window.refresh(); window.draw(cx).clear(cx); }).unwrap();
                    }).await;
                    let _ = finished.send(());
                }).detach();
                cx.new(|cx| Root::new(view, window, cx))
            }).unwrap();
            completion.await.expect("native file preview fixture closed before completion");
            }).await;
            cx.update(|cx| cx.quit());
        }).detach();
    });
}

fn verify_unattached(view: &Entity<Harness>, cx: &mut AsyncWindowContext) {
    cx.update(|_, cx| {
        let view = view.read(cx);
        let page = &view.native.as_ref().unwrap().page;
        page.webview().removeFromSuperview();
        assert!(
            pointer::Pointer::attach(page).is_err(),
            "an unattached preview must fail without panicking"
        );
    })
    .unwrap();
}

async fn verify_documents(
    view: &Entity<Harness>,
    documents: Vec<(String, Vec<u8>, String)>,
    cx: &mut AsyncWindowContext,
) {
    documents::assets::bundle().unwrap();
    for (name, bytes, expected) in documents {
        let mime = mime_guess::from_path(&name).first_raw().unwrap();
        documents::assets::validate(mime, &bytes).unwrap();
        let text_check = if mime == "application/pdf" {
            format!(
                "document.querySelector('canvas')?.width > 0 && [...document.querySelectorAll('.textLayer')].some(layer => layer.textContent.includes({}))",
                serde_json::to_string(&expected).unwrap()
            )
        } else {
            format!(
                "[...document.querySelectorAll('.preview-page')].some(page => page.offsetWidth > 0 && page.offsetHeight > 0 && page.textContent.includes({}))",
                serde_json::to_string(&expected).unwrap()
            )
        };
        let status = Rc::new(std::cell::RefCell::new(String::new()));
        let received = status.clone();
        let previous = view.update(cx, |view, _| view.native.clone().unwrap());
        let native = cx
            .update(|window, _| {
                documents::assets::native(
                    bytes.into(),
                    mime,
                    "#dddddd",
                    move |body| {
                        *received.borrow_mut() = body.to_owned();
                    },
                    window,
                )
                .unwrap()
            })
            .unwrap();
        view.update(cx, |view, cx| {
            view.native = Some(Rc::new(native));
            cx.notify();
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            pause(cx).await;
            let value: serde_json::Value =
                serde_json::from_str(&status.borrow()).unwrap_or_default();
            assert!(
                value.get("error").is_none(),
                "{name} preview failed: {value}"
            );
            if value["pages"] == 2 && evaluate(view, &text_check, cx).await == "true" {
                break;
            }
            if std::time::Instant::now() >= deadline {
                let detail = evaluate(view, "JSON.stringify({canvases:[...document.querySelectorAll('canvas')].map(c=>[c.width,c.height]),text:[...document.querySelectorAll('.textLayer')].map(layer=>layer.textContent),ready:document.readyState,visibility:document.visibilityState,layout:[...document.querySelectorAll('#preview,#container,#pages,.page')].map(e=>({name:e.id||e.className,rect:e.getBoundingClientRect().toJSON(),height:e.clientHeight,width:e.clientWidth}))})", cx).await;
                panic!("{name} preview deadline, status: {value}, content: {detail}");
            }
        }
        let old_hidden: bool = unsafe { objc2::msg_send![&*previous.page.webview(), isHidden] };
        assert!(old_hidden, "replacing a preview hides its old native view");
        drop(previous);
        let status_before: serde_json::Value = serde_json::from_str(&status.borrow()).unwrap();
        evaluate(view, "window.previewAction('in');true", cx).await;
        pause(cx).await;
        let status_after: serde_json::Value = serde_json::from_str(&status.borrow()).unwrap();
        assert!(status_after["scale"].as_f64().unwrap() > status_before["scale"].as_f64().unwrap());
        evaluate(view, "window.previewAction('next');true", cx).await;
        pause(cx).await;
        let status_after: serde_json::Value = serde_json::from_str(&status.borrow()).unwrap();
        assert_eq!(status_after["page"], 2);
        evaluate(
            view,
            "window.previewAction('previous');window.previewAction('fit');true",
            cx,
        )
        .await;
        pause(cx).await;
        check_pointer(view, cx).await;
        view.update(cx, |view, cx| {
            view.mounted = false;
            cx.notify();
        });
        pause(cx).await;
        assert!(hidden(view, cx));
        view.update(cx, |view, cx| {
            view.mounted = true;
            cx.notify();
        });
        pause(cx).await;
        assert!(!hidden(view, cx));
        println!(
            "Verified {name}: rendered pages, zoom, navigation, replacement, host hover and unmount"
        );
    }
}

async fn pause(cx: &mut AsyncWindowContext) {
    cx.background_executor()
        .timer(Duration::from_millis(200))
        .await;
    // Native tests must finish a frame even when AppKit throttles background
    // windows or the display is asleep. Timers alone do not commit a GPUI draw.
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    })
    .unwrap();
}

async fn evaluate(view: &Entity<Harness>, script: &str, cx: &mut AsyncWindowContext) -> String {
    let (send, receive) = tokio::sync::oneshot::channel();
    let send = std::sync::Mutex::new(Some(send));
    cx.update(|_, cx| {
        view.read(cx)
            .native
            .as_ref()
            .unwrap()
            .page
            .evaluate_script_with_callback(script, move |value| {
                if let Some(send) = send.lock().unwrap().take() {
                    let _ = send.send(value);
                }
            })
            .unwrap()
    })
    .unwrap();
    receive.await.unwrap()
}

fn hidden(view: &Entity<Harness>, cx: &mut AsyncWindowContext) -> bool {
    cx.update(|_, cx| {
        let page = view.read(cx).native.as_ref().unwrap().page.webview();
        unsafe { objc2::msg_send![&*page, isHidden] }
    })
    .unwrap()
}

async fn verify(view: &Entity<Harness>, cx: &mut AsyncWindowContext) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        pause(cx).await;
        let loaded = cx
            .update(|_, cx| {
                let page = view.read(cx).native.as_ref().unwrap().page.webview();
                // SAFETY: Wry retains this view, and GPUI runs the update on its AppKit thread.
                unsafe { !page.isLoading() && page.estimatedProgress() == 1. }
            })
            .unwrap();
        if loaded {
            break;
        }
        // Pinned Wry queues pre-commit scripts without retaining their callbacks.
        assert!(
            std::time::Instant::now() < deadline,
            "HTML fixture did not finish loading"
        );
    }
    evaluate(view, "window.fixtureReady=false;window.fixtureClicked=false;addEventListener('message',e=>{if(e.data==='fixture-ready')window.fixtureReady=true;if(e.data==='fixture-clicked')window.fixtureClicked=true;});true", cx).await;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while evaluate(view, "window.fixtureReady", cx).await != "true" {
        assert!(
            std::time::Instant::now() < deadline,
            "HTML fixture did not load"
        );
        pause(cx).await;
    }
    evaluate(
        view,
        "document.querySelector('iframe').contentWindow.postMessage('fixture-click','*');true",
        cx,
    )
    .await;
    pause(cx).await;
    assert_eq!(evaluate(view, "window.fixtureClicked", cx).await, "true");
    assert_eq!(
        evaluate(view, "document.querySelector('iframe').style.top", cx).await,
        "\"-40px\""
    );
    assert!(!hidden(view, cx));
    evaluate(view, "window.layoutCount=0;const layout=window.layoutPreview;window.layoutPreview=()=>{window.layoutCount++;layout()};true", cx).await;
    for _ in 0..3 {
        cx.update(|window, _| window.refresh()).unwrap();
        pause(cx).await;
    }
    assert_eq!(
        evaluate(view, "window.layoutCount", cx).await,
        "0",
        "unchanged host frames do not schedule WebKit layout scripts"
    );
    cx.update(|_, cx| {
        let native = view.read(cx).native.as_ref().unwrap();
        let (bounds, clip) = native.frame.get().unwrap();
        assert_eq!(bounds.size.height, px(180.));
        assert_eq!(clip.size.height, px(100.));
        assert_eq!(
            native
                .page
                .bounds()
                .unwrap()
                .size
                .to_logical::<f64>(1.)
                .height,
            100.
        );
    })
    .unwrap();
    verify_freshness(view, cx).await;
    cx.update(|window, cx| {
        view.read(cx).native.as_ref().unwrap().page.focus().unwrap();
        window.open_dialog(cx, |dialog, _, _| dialog.title("Preview fixture"));
        window.refresh();
    })
    .unwrap();
    pause(cx).await;
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx))
            .unwrap()
    );
    assert!(hidden(view, cx));
    assert!(
        view.read_with(cx, |view, _| view.native.as_ref().unwrap().substituted()),
        "an overlapping modal keeps native content visible through its snapshot"
    );
    cx.update(|_, cx| {
        let page = view.read(cx).native.as_ref().unwrap().page.webview();
        let parent = unsafe { page.superview() }.unwrap();
        assert_eq!(
            parent.window().unwrap().firstResponder().as_deref(),
            Some(&**parent as &objc2_app_kit::NSResponder),
            "a hidden preview releases native keyboard focus"
        );
    })
    .unwrap();
    cx.update(|window, cx| {
        window.close_dialog(cx);
        window.refresh();
    })
    .unwrap();
    pause(cx).await;
    assert!(!hidden(view, cx));
    verify_surfaces(view, cx).await;
    println!(
        "File preview: clipped modal, menu, toast and event-driven load/resize/scroll cache verified"
    );
    check_pointer(view, cx).await;
    view.update(cx, |view, cx| {
        view.crop = false;
        cx.notify();
    });
    pause(cx).await;
    assert_eq!(
        evaluate(view, "document.querySelector('iframe').style.top", cx).await,
        "\"0px\""
    );
    view.update(cx, |view, cx| {
        view.mounted = false;
        cx.notify();
    });
    pause(cx).await;
    assert!(hidden(view, cx));
    view.update(cx, |view, cx| {
        view.mounted = true;
        cx.notify();
    });
    pause(cx).await;
    assert!(!hidden(view, cx));
}

async fn fresh(view: &Entity<Harness>, previous: Option<u64>, cx: &mut AsyncWindowContext) -> u64 {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let (generation, current) =
            view.read_with(cx, |view, _| view.native.as_ref().unwrap().cache_state());
        if current && previous.is_none_or(|previous| generation > previous) {
            return generation;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "native snapshot did not refresh after content settled"
        );
        pause(cx).await;
    }
}

async fn verify_freshness(view: &Entity<Harness>, cx: &mut AsyncWindowContext) {
    let loaded = fresh(view, None, cx).await;
    assert!(
        loaded > 1,
        "native page completion must invalidate the initial layout cache"
    );
    evaluate(
        view,
        "document.querySelector('iframe').contentWindow.postMessage('fixture-scroll','*');true",
        cx,
    )
    .await;
    let scrolled = fresh(view, Some(loaded), cx).await;
    view.update(cx, |view, cx| {
        view.height = px(200.);
        cx.notify();
    });
    pause(cx).await;
    let resized = fresh(view, Some(scrolled), cx).await;
    assert!(
        !hidden(view, cx),
        "a visible page stays native while its cache warms"
    );
    view.update(cx, |view, cx| {
        view.height = px(180.);
        cx.notify();
    });
    pause(cx).await;
    fresh(view, Some(resized), cx).await;
}

async fn verify_surfaces(view: &Entity<Harness>, cx: &mut AsyncWindowContext) {
    for (left, covered) in [(px(320.), false), (px(20.), true)] {
        view.update(cx, |view, cx| {
            view.overlay = Some(left);
            cx.notify();
        });
        pause(cx).await;
        assert_eq!(hidden(view, cx), covered);
        assert_eq!(
            view.read_with(cx, |view, _| view.native.as_ref().unwrap().substituted()),
            covered
        );
    }
    view.update(cx, |view, cx| {
        view.overlay = None;
        cx.notify();
    });
    pause(cx).await;
    for (placement, covered) in [(Anchor::TopRight, false), (Anchor::TopLeft, true)] {
        cx.update(|window, cx| {
            window.push_notification(
                notification::Notification::info("Preview fixture")
                    .placement(placement)
                    .autohide(false),
                cx,
            );
        })
        .unwrap();
        pause(cx).await;
        assert_eq!(hidden(view, cx), covered);
        assert_eq!(
            view.read_with(cx, |view, _| view.native.as_ref().unwrap().substituted()),
            covered
        );
        cx.update(|window, cx| window.clear_notifications(cx))
            .unwrap();
        pause(cx).await;
        pause(cx).await;
    }
    assert!(!hidden(view, cx));
    assert!(!view.read_with(cx, |view, _| view.native.as_ref().unwrap().substituted()));
}

async fn check_pointer(view: &Entity<Harness>, cx: &mut AsyncWindowContext) {
    cx.update(|window, cx| {
        cx.activate(true);
        window.activate_window();
        window.refresh();
    })
    .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        pause(cx).await;
        let (active, _, _, detail) = pointer_state(view, cx);
        if active {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "pointer fixture did not become active: {detail}"
        );
    }
    let target = cx
        .update(|_, cx| {
            let view = view.read(cx);
            view.native.as_ref().unwrap().page.focus().unwrap();
            view.trigger.get().center()
        })
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        pause(cx).await;
        let (active, focused, _, detail) = pointer_state(view, cx);
        if active && focused {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "pointer fixture did not retain WebKit focus: {detail}"
        );
    }
    wait_hover(
        view,
        target,
        true,
        "host hover works while WebKit owns keyboard focus",
        cx,
    )
    .await;
    wait_hover(
        view,
        point(px(10.), px(10.)),
        false,
        "moving into HTML clears host hover",
        cx,
    )
    .await;
}

async fn wait_hover(
    view: &Entity<Harness>,
    target: Point<Pixels>,
    expected: bool,
    message: &str,
    cx: &mut AsyncWindowContext,
) {
    let before = pointer_state(view, cx).3;
    move_pointer(view, target, cx);
    let immediate = pointer_state(view, cx).3;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        pause(cx).await;
        let (active, focused, hovered, detail) = pointer_state(view, cx);
        if active && focused && hovered == expected {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{message}: target={target:?}, before={before}, immediate={immediate}, after={detail}"
        );
    }
}

fn pointer_state(
    view: &Entity<Harness>,
    cx: &mut AsyncWindowContext,
) -> (bool, bool, bool, String) {
    cx.update(|window, cx| {
        let view = view.read(cx);
        let page = view.native.as_ref().unwrap().page.webview();
        let native = page.window().unwrap();
        let responder = native.firstResponder();
        let focused = responder
            .as_deref()
            .and_then(|responder| responder.downcast_ref::<NSView>())
            .is_some_and(|focused| focused.isDescendantOf(&page));
        let key = native.isKeyWindow();
        let active = window.is_window_active();
        let hovered = view.hovered.get();
        let detail = format!(
            "key={key}, main={}, active={active}, focused={focused}, hidden={}, first_responder={responder:?}, cursor={:?}, physical_cursor={:?}, keyboard={}, trigger={:?}, hovered={hovered}",
            native.isMainWindow(),
            page.isHidden(),
            window.mouse_position(),
            native.mouseLocationOutsideOfEventStream(),
            window.last_input_was_keyboard(),
            view.trigger.get(),
        );
        (key && active, focused, hovered, detail)
    })
    .unwrap()
}

fn move_pointer(view: &Entity<Harness>, position: Point<Pixels>, cx: &mut AsyncWindowContext) {
    let (window, event) = cx.update(|_, cx| {
        let page = view.read(cx).native.as_ref().unwrap().page.webview();
        let parent = unsafe { page.superview() }.unwrap();
        let window = parent.window().unwrap();
        let y = if parent.isFlipped() { f64::from(f32::from(position.y)) }
            else { parent.bounds().size.height - f64::from(f32::from(position.y)) };
        let location = parent.convertPoint_toView(NSPoint::new(f64::from(f32::from(position.x)), y), None);
        let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            NSEventType::MouseMoved, location, NSEventModifierFlags::empty(), 0., window.windowNumber(), None, 0, 0, 0.
        ).unwrap();
        (window, event)
    }).unwrap();
    window.sendEvent(&event);
}
