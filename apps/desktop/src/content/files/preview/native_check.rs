//! Native WebKit acceptance uses an isolated HTML fixture, never user files or screenshots.
use super::*;
use gpui_kit::{prelude::FluentBuilder, test::TestWindowExt};
use objc2_app_kit::NSView;
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
    clicks: Rc<Cell<usize>>,
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
                let clicks = self.clicks.clone();
                this.child(
                    div().absolute().left(left).top_0().child(
                        Button::new("preview-menu-trigger")
                            .label("Menu")
                            .dropdown_menu(move |menu, _, _| {
                                let clicks = clicks.clone();
                                menu.item(PopupMenuItem::new("Choose").on_click(move |_, _, _| {
                                    clicks.set(clicks.get() + 1);
                                }))
                            }),
                    ),
                )
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
        crate::workload::native_overlays::init(cx);
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
                let native = Rc::new(surface::Native::new(HTML, window, cx).unwrap());
                let view = cx.new(|_| Harness { native: Some(native), mounted: true, crop: true, hovered: Rc::default(), trigger: Rc::default(), overlay: None, clicks: Rc::default(), height: px(180.), width: px(300.), viewport: px(100.) });
                let probe = view.clone();
                window.spawn(cx, async move |cx| {
                    let _ = crate::workload::check(async {
                    verify(&probe, &output, cx).await;
                    verify_documents(&probe, documents, cx).await;
                    std::fs::write(output.join("file-preview.json"), "{\"controlled_frames\":true,\"webkit\":true,\"javascript\":true,\"host_hover\":true,\"clipping\":true,\"overlays\":true,\"unmount\":true,\"pdf\":true,\"docx\":true,\"xlsx\":true,\"pptx\":true,\"pagination\":true,\"zoom\":true}\n").unwrap();
                    println!("File preview: HTML, PDF, DOCX, XLSX, PPTX, pagination, zoom, host hover, clipping, overlays and unmount verified");
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
            .update(|window, cx| {
                documents::assets::native(
                    bytes.into(),
                    mime,
                    "#dddddd",
                    move |body| {
                        *received.borrow_mut() = body.to_owned();
                    },
                    window,
                    cx,
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
        window.render_frame(cx);
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

async fn verify(view: &Entity<Harness>, output: &Path, cx: &mut AsyncWindowContext) {
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
                .raw()
                .bounds()
                .unwrap()
                .size
                .to_logical::<f64>(1.)
                .height,
            100.
        );
    })
    .unwrap();
    verify_layout(view, cx).await;
    let clicks = view.read_with(cx, |view, _| view.clicks.clone());
    let action = clicks.clone();
    cx.update(|window, cx| {
        view.read(cx).native.as_ref().unwrap().page.focus().unwrap();
        window.open_dialog(cx, move |dialog, _, _| {
            let action = action.clone();
            dialog.title("Preview fixture").child(
                Button::new("preview-dialog-action")
                    .label("Choose")
                    .on_click(move |_, _, _| {
                        action.set(action.get() + 1);
                    }),
            )
        });
        window.refresh();
    })
    .unwrap();
    pause(cx).await;
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx))
            .unwrap()
    );
    assert!(
        !hidden(view, cx),
        "native composition retains the live preview below a modal"
    );
    cx.update(|window, cx| window.click("preview-dialog-action", cx))
        .unwrap();
    assert_eq!(
        clicks.get(),
        1,
        "the real Kit dialog receives a click above native content"
    );
    assert!(
        !pointer_state(view, cx).1,
        "the dialog action releases WebKit keyboard focus"
    );
    cx.update(|window, cx| {
        window.close_dialog(cx);
        window.refresh();
    })
    .unwrap();
    pause(cx).await;
    assert!(!hidden(view, cx));
    verify_surfaces(view, cx).await;
    let page = cx
        .update(|_, cx| view.read(cx).native.as_ref().unwrap().page.handle())
        .unwrap();
    crate::workload::native_overlays::verify("file-preview", page, output, cx).await;
    println!(
        "File preview: native clipping, modal/menu/toast interaction and load/resize/scroll verified"
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

async fn verify_layout(view: &Entity<Harness>, cx: &mut AsyncWindowContext) {
    evaluate(view, "window.fixtureScrolled=false;addEventListener('message',e=>{if(e.data==='fixture-scrolled')window.fixtureScrolled=true;});document.querySelector('iframe').contentWindow.postMessage('fixture-scroll','*');true", cx).await;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while evaluate(view, "window.fixtureScrolled", cx).await != "true" {
        assert!(
            std::time::Instant::now() < deadline,
            "the native document did not acknowledge scrolling"
        );
        pause(cx).await;
    }
    for height in [px(200.), px(180.)] {
        view.update(cx, |view, cx| {
            view.height = height;
            cx.notify();
        });
        pause(cx).await;
        assert!(!hidden(view, cx), "resizing keeps the native page visible");
        assert_eq!(
            evaluate(view, "document.querySelector('iframe').style.height", cx).await,
            format!("\"{}px\"", f32::from(height))
        );
        cx.update(|_, cx| {
            let native = view.read(cx).native.as_ref().unwrap();
            let (bounds, clip) = native.frame.get().unwrap();
            assert_eq!(bounds.size.height, height);
            assert_eq!(clip.size.height, px(100.));
            assert_eq!(
                native
                    .page
                    .raw()
                    .bounds()
                    .unwrap()
                    .size
                    .to_logical::<f64>(1.)
                    .height,
                100.
            );
        })
        .unwrap();
    }
}

async fn verify_surfaces(view: &Entity<Harness>, cx: &mut AsyncWindowContext) {
    let clicks = view.read_with(cx, |view, _| view.clicks.clone());
    for (left, covered) in [(px(320.), false), (px(20.), true)] {
        view.update(cx, |view, cx| {
            view.overlay = Some(left);
            cx.notify();
        });
        pause(cx).await;
        let previous = clicks.get();
        cx.update(|window, cx| window.click("preview-menu-trigger", cx))
            .unwrap();
        pause(cx).await;
        cx.update(|window, cx| {
            let menu = window.find("popup-menu").bounds();
            let clip = view
                .read(cx)
                .native
                .as_ref()
                .unwrap()
                .frame
                .get()
                .unwrap()
                .1;
            let overlap = clip.intersect(&menu);
            assert_eq!(
                overlap.size.width > px(0.) && overlap.size.height > px(0.),
                covered
            );
            if covered {
                assert!(
                    clip.contains(&window.within("popup-menu").find(0usize).bounds().center()),
                    "the chosen menu item overlaps the native page"
                );
            }
            window.within("popup-menu").click(0usize, cx);
        })
        .unwrap();
        assert_eq!(
            clicks.get(),
            previous + 1,
            "the real Kit menu receives the selected action"
        );
        assert!(
            !hidden(view, cx),
            "native content remains visible beneath the menu"
        );
    }
    view.update(cx, |view, cx| {
        view.overlay = None;
        cx.notify();
    });
    pause(cx).await;
    for (placement, covered) in [(Anchor::TopRight, false), (Anchor::TopLeft, true)] {
        let action = clicks.clone();
        cx.update(|window, cx| {
            window.push_notification(
                notification::Notification::info("Preview")
                    .placement(placement)
                    .autohide(false)
                    .action(move |_, _, _| {
                        let action = action.clone();
                        Button::new("preview-toast-action")
                            .label("Choose")
                            .on_click(move |_, _, _| {
                                action.set(action.get() + 1);
                            })
                    }),
                cx,
            );
        })
        .unwrap();
        crate::workload::native_overlays::wait_visible("preview-toast-action", cx).await;
        let previous = clicks.get();
        cx.update(|window, cx| {
            let toast = window.find("notification").bounds();
            let clip = view
                .read(cx)
                .native
                .as_ref()
                .unwrap()
                .frame
                .get()
                .unwrap()
                .1;
            let overlap = clip.intersect(&toast);
            assert_eq!(
                overlap.size.width > px(0.) && overlap.size.height > px(0.),
                covered
            );
            if covered {
                assert!(
                    clip.contains(&window.find("preview-toast-action").bounds().center()),
                    "the toast action overlaps the native page"
                );
            }
            window.click("preview-toast-action", cx);
        })
        .unwrap();
        assert_eq!(
            clicks.get(),
            previous + 1,
            "the real Kit toast action receives a click"
        );
        assert!(
            !hidden(view, cx),
            "native content remains visible beneath the toast"
        );
        cx.update(|window, cx| window.clear_notifications(cx))
            .unwrap();
        pause(cx).await;
        pause(cx).await;
    }
    assert!(!hidden(view, cx));
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
    let page = cx
        .update(|_, cx| view.read(cx).native.as_ref().unwrap().page.handle())
        .unwrap();
    crate::workload::native_overlays::move_pointer(&page, position);
}
