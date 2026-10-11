//! Controlled native overlay checks, separate from frame/performance measurements.
use gpui_kit::{
    base::{Root, RootPlugin},
    component::{
        Placement, WindowExt,
        button::Button,
        h_flex,
        menu::{DropdownMenu, PopupMenuItem},
        notification::Notification,
        v_flex,
    },
    prelude::FluentBuilder,
    test::{ElementSnapshot, TestWindowExt},
    *,
};
use gpui_webview::WebViewHandle;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSScreen, NSView};
use objc2_core_graphics::{
    CGAssociateMouseAndMouseCursorPosition, CGError, CGEvent, CGEventField, CGEventFlags,
    CGEventTapLocation, CGEventType, CGMouseButton, CGWarpMouseCursorPosition,
};
use objc2_foundation::NSPoint;
use std::{
    cell::Cell,
    path::Path,
    rc::Rc,
    time::{Duration, Instant},
};
use wry::WebViewExtMacOS;

#[derive(Default)]
struct Probe {
    position: Option<Point<Pixels>>,
    clicks: Rc<Cell<usize>>,
    inset_probe: bool,
}
impl RootPlugin for Probe {}
impl Render for Probe {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Registered before Component's WindowState: Shell has rendered its
        // normal insets, and the real notification layer is built after this.
        if self.inset_probe {
            window.set_notification_insets(Edges::default(), cx);
        }
        let clicks = self.clicks.clone();
        deferred(div().when_some(self.position, |this, position| {
            let action = clicks.clone();
            this.absolute().left(position.x).top(position.y).child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("native-tooltip-action")
                            .label("Choose")
                            .tooltip("Native fixture")
                            .tooltip_placement(Placement::Bottom)
                            .on_click(move |_, _, _| action.set(action.get() + 1)),
                    )
                    .child(
                        Button::new("native-menu-trigger")
                            .label("Menu")
                            .dropdown_menu(move |menu, _, _| {
                                let action = clicks.clone();
                                menu.item(PopupMenuItem::new("Choose").on_click(move |_, _, _| {
                                    action.set(action.get() + 1);
                                }))
                            }),
                    ),
            )
        }))
    }
}

pub(crate) fn init(cx: &mut App) {
    if !std::env::var("SAILRY_WORKLOAD_CASE").is_ok_and(|case| {
        matches!(
            case.as_str(),
            "file-preview" | "html-brief" | "html-brief-flow" | "browser" | "browser-interactions"
        )
    }) {
        return;
    }
    Root::register_plugin(cx, |_, _| Probe::default());
}

#[derive(Clone, Copy)]
enum Target {
    Element(&'static str),
    MenuItem,
}
fn query(window: &mut Window, target: Target) -> Option<ElementSnapshot> {
    match target {
        Target::Element(id) => window.try_find(id),
        Target::MenuItem => {
            window.try_find("popup-menu")?;
            window.within("popup-menu").try_find(0usize)
        }
    }
}
async fn frame(cx: &mut AsyncWindowContext) {
    cx.background_executor()
        .timer(Duration::from_millis(20))
        .await;
    cx.update(|window, cx| window.render_frame(cx)).unwrap();
}
async fn visible(target: Target, cx: &mut AsyncWindowContext) -> Bounds<Pixels> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut previous = None;
    let mut stable = 0;
    loop {
        frame(cx).await;
        let bounds = cx
            .update(|window, _| {
                query(window, target)
                    .filter(|item| item.visible() && item.disabled() != Some(true))
                    .map(|item| item.bounds())
                    .filter(|bounds| bounds.size.width > px(0.) && bounds.size.height > px(0.))
            })
            .unwrap();
        if bounds.is_some() && bounds == previous {
            stable += 1;
            if stable >= 2 {
                return bounds.unwrap();
            }
        } else {
            stable = 0;
        }
        previous = bounds;
        assert!(
            Instant::now() < deadline,
            "native overlay target did not become visible and stable: {bounds:?}"
        );
    }
}
pub(crate) async fn wait_visible(id: &'static str, cx: &mut AsyncWindowContext) -> Bounds<Pixels> {
    visible(Target::Element(id), cx).await
}
async fn absent(id: &'static str, cx: &mut AsyncWindowContext) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        frame(cx).await;
        if cx
            .update(|window, _| window.try_find(id).is_none_or(|item| !item.visible()))
            .unwrap()
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "native overlay did not close: {id}"
        );
    }
}
fn native_bounds(page: &WebViewHandle) -> Bounds<Pixels> {
    let view = page.raw().webview();
    assert!(
        !view.isHiddenOrHasHiddenAncestor(),
        "native page must remain visible below Kit overlays"
    );
    let native = view
        .window()
        .expect("native page must stay attached to its window");
    let content = native.contentView().unwrap();
    let rect = view.convertRect_toView(view.bounds(), Some(&content));
    let y = if content.isFlipped() {
        rect.origin.y
    } else {
        content.bounds().size.height - rect.origin.y - rect.size.height
    };
    let bounds = Bounds::new(
        point(px(rect.origin.x as f32), px(y as f32)),
        size(px(rect.size.width as f32), px(rect.size.height as f32)),
    );
    assert!(
        bounds.size.width > px(0.) && bounds.size.height > px(0.),
        "native page has empty bounds: {bounds:?}"
    );
    bounds
}
fn native_focused(page: &WebViewHandle) -> bool {
    let view = page.raw().webview();
    view.window()
        .unwrap()
        .firstResponder()
        .as_deref()
        .and_then(|responder| responder.downcast_ref::<NSView>())
        .is_some_and(|responder| responder.isDescendantOf(&view))
}
fn overlap(native: Bounds<Pixels>, overlay: Bounds<Pixels>) -> bool {
    let intersection = native.intersect(&overlay);
    intersection.size.width > px(0.) && intersection.size.height > px(0.)
}
fn covered(page: &WebViewHandle, bounds: Bounds<Pixels>) {
    let native = native_bounds(page);
    assert!(
        overlap(native, bounds),
        "real Kit overlay must overlap the native page: native={native:?}, overlay={bounds:?}"
    );
    assert!(
        native.contains(&bounds.center()),
        "OS action point must lie over native content: native={native:?}, target={bounds:?}"
    );
}
fn location(page: &WebViewHandle, position: Point<Pixels>) -> NSPoint {
    // Reuse the file-preview pointer fixture's AppKit-to-Quartz conversion.
    let view = page.raw().webview();
    let window = view.window().unwrap();
    let content = window.contentView().unwrap();
    let y = if content.isFlipped() {
        f64::from(f32::from(position.y))
    } else {
        content.bounds().size.height - f64::from(f32::from(position.y))
    };
    let local =
        content.convertPoint_toView(NSPoint::new(f64::from(f32::from(position.x)), y), None);
    let screen = window.convertPointToScreen(local);
    let primary = NSScreen::screens(MainThreadMarker::new().unwrap())
        .firstObject()
        .unwrap();
    NSPoint::new(screen.x, primary.frame().size.height - screen.y)
}
fn post(page: &WebViewHandle, position: Point<Pixels>, kind: CGEventType) {
    let location = location(page, position);
    assert_eq!(CGWarpMouseCursorPosition(location), CGError::Success);
    assert_eq!(
        CGAssociateMouseAndMouseCursorPosition(true),
        CGError::Success
    );
    let event = CGEvent::new_mouse_event(None, kind, location, CGMouseButton::Left)
        .expect("cannot create native pointer event");
    // The isolated fixture requests an ordinary unmodified pointer gesture.
    // This changes only the event packet, not the physical keyboard state.
    CGEvent::set_flags(Some(&event), CGEventFlags::empty());
    if kind == CGEventType::LeftMouseDown || kind == CGEventType::LeftMouseUp {
        // Quartz defaults the newly created up event's click state to zero.
        // GPUI reads the up event's count; both halves describe one real click.
        CGEvent::set_integer_value_field(Some(&event), CGEventField::MouseEventClickState, 1);
    }
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
}
pub(crate) fn move_pointer(page: &WebViewHandle, position: Point<Pixels>) {
    post(page, position, CGEventType::MouseMoved);
}
async fn move_to(page: &WebViewHandle, position: Point<Pixels>, cx: &mut AsyncWindowContext) {
    move_pointer(page, position);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        frame(cx).await;
        let current = cx
            .update(|window, _| {
                native_bounds(page);
                window.mouse_position()
            })
            .unwrap();
        if (current.x - position.x).abs() < px(2.) && (current.y - position.y).abs() < px(2.) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "OS pointer was not delivered to Kit: requested={position:?}, observed={current:?}"
        );
    }
}
async fn click(
    page: &WebViewHandle,
    target: Target,
    clicks: Option<&Cell<usize>>,
    cx: &mut AsyncWindowContext,
) {
    let bounds = visible(target, cx).await;
    cx.update(|_, _| covered(page, bounds)).unwrap();
    move_to(page, bounds.center(), cx).await;
    // Hover can change the mounted control; requery before the physical press.
    let hovered = visible(target, cx).await;
    cx.update(|_, _| covered(page, hovered)).unwrap();
    move_to(page, hovered.center(), cx).await;
    let previous = clicks.map(Cell::get);
    post(page, hovered.center(), CGEventType::LeftMouseDown);
    frame(cx).await;
    post(page, hovered.center(), CGEventType::LeftMouseUp);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        frame(cx).await;
        cx.update(|_, _| {
            native_bounds(page);
        })
        .unwrap();
        if clicks.is_none_or(|counter| counter.get() == previous.unwrap() + 1) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "OS click did not deliver exactly one Kit action: before={previous:?}, after={:?}",
            clicks.map(Cell::get)
        );
    }
}
async fn activate(page: &WebViewHandle, cx: &mut AsyncWindowContext) {
    cx.update(|window, cx| {
        cx.activate(true);
        window.activate_window();
    })
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        frame(cx).await;
        let active = cx
            .update(|window, _| {
                native_bounds(page);
                let native = page.raw().webview().window().unwrap();
                native.isKeyWindow() && window.is_window_active()
            })
            .unwrap();
        if active {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "native overlay fixture did not become active; actual OS input is required"
        );
    }
}
fn push_toast(
    page: &WebViewHandle,
    right: bool,
    clicks: Rc<Cell<usize>>,
    window: &mut Window,
    cx: &mut App,
) {
    let native = native_bounds(page);
    window.push_notification(
        Notification::info("Native fixture")
            .placement(if right {
                Anchor::TopRight
            } else {
                Anchor::TopLeft
            })
            .w(native.size.width.min(px(260.)))
            .mt(native.top())
            .autohide(false)
            .action(move |_, _, _| {
                let action = clicks.clone();
                Button::new("native-toast-action")
                    .label("Choose")
                    .on_click(move |_, _, _| action.set(action.get() + 1))
            }),
        cx,
    );
}
async fn clear_toast(cx: &mut AsyncWindowContext) {
    cx.update(|window, cx| window.clear_notifications(cx))
        .unwrap();
    absent("native-toast-action", cx).await;
}

pub(crate) async fn verify(
    name: &str,
    page: WebViewHandle,
    output: &Path,
    cx: &mut AsyncWindowContext,
) {
    activate(&page, cx).await;
    let (probe, native, right) = cx
        .update(|window, cx| {
            let native = native_bounds(&page);
            let right = native.center().x > window.viewport_size().width / 2.;
            let probe = Root::read(window, cx).plugin::<Probe>().unwrap();
            probe.update(cx, |probe, cx| {
                probe.position = Some(native.origin + point(px(20.), px(24.)));
                cx.notify();
            });
            (probe, native, right)
        })
        .unwrap();
    let clicks = probe.read_with(cx, |probe, _| probe.clicks.clone());
    let initial_clicks = clicks.get();
    let tooltip_button = wait_visible("native-tooltip-action", cx).await;
    cx.update(|_, _| {
        page.raw().focus().unwrap();
    })
    .unwrap();
    move_to(&page, tooltip_button.center(), cx).await;
    let tooltip = wait_visible("managed-tooltip-popup", cx).await;
    cx.update(|_, _| {
        assert!(
            overlap(native_bounds(&page), tooltip),
            "real managed tooltip must overlap the native page"
        );
        assert!(
            native_focused(&page),
            "native keyboard focus must survive OS hover over Kit"
        );
    })
    .unwrap();
    click(
        &page,
        Target::Element("native-tooltip-action"),
        Some(clicks.as_ref()),
        cx,
    )
    .await;
    click(&page, Target::Element("native-menu-trigger"), None, cx).await;
    let menu = wait_visible("popup-menu", cx).await;
    cx.update(|_, _| assert!(overlap(native_bounds(&page), menu)))
        .unwrap();
    click(&page, Target::MenuItem, Some(clicks.as_ref()), cx).await;
    absent("popup-menu", cx).await;
    let action = clicks.clone();
    cx.update(|window, cx| {
        page.raw().focus().unwrap();
        let width = window.viewport_size().width;
        window.open_dialog(cx, move |dialog, _, _| {
            let action = action.clone();
            dialog.width(width).margin_top(native.top()).child(
                v_flex()
                    .w_full()
                    .when(right, |row| row.items_end())
                    .when(!right, |row| row.items_start())
                    .child(
                        Button::new("native-dialog-action")
                            .label("Choose")
                            .on_click(move |_, _, _| action.set(action.get() + 1)),
                    ),
            )
        });
    })
    .unwrap();
    let dialog = wait_visible("dialog", cx).await;
    cx.update(|_, _| assert!(overlap(native_bounds(&page), dialog)))
        .unwrap();
    click(
        &page,
        Target::Element("native-dialog-action"),
        Some(clicks.as_ref()),
        cx,
    )
    .await;
    cx.update(|window, cx| {
        assert!(
            !native_focused(&page),
            "the real Kit dialog must release native keyboard focus"
        );
        window.close_dialog(cx);
    })
    .unwrap();
    absent("dialog", cx).await;

    // Record the ordinary Shell placement, then test backend compositing over
    // the viewport with zero reservation. This does not claim Browser's normal
    // reserved toast position overlaps its page. Repeating the ordinary toast
    // after disabling the probe proves the Shell's placement was restored.
    cx.update(|window, cx| push_toast(&page, right, clicks.clone(), window, cx))
        .unwrap();
    wait_visible("native-toast-action", cx).await;
    let normal = wait_visible("notification", cx).await;
    let normal_overlap = cx
        .update(|_, _| overlap(native_bounds(&page), normal))
        .unwrap();
    clear_toast(cx).await;
    probe.update(cx, |probe, cx| {
        probe.inset_probe = true;
        cx.notify();
    });
    frame(cx).await;
    cx.update(|window, cx| push_toast(&page, right, clicks.clone(), window, cx))
        .unwrap();
    let toast = wait_visible("notification", cx).await;
    cx.update(|_, _| assert!(overlap(native_bounds(&page), toast)))
        .unwrap();
    click(
        &page,
        Target::Element("native-toast-action"),
        Some(clicks.as_ref()),
        cx,
    )
    .await;
    clear_toast(cx).await;
    probe.update(cx, |probe, cx| {
        probe.inset_probe = false;
        cx.notify();
    });
    frame(cx).await;
    cx.update(|window, cx| push_toast(&page, right, clicks.clone(), window, cx))
        .unwrap();
    wait_visible("native-toast-action", cx).await;
    let restored = wait_visible("notification", cx).await;
    assert_eq!(
        restored, normal,
        "normal Shell toast placement must be restored after the controlled probe"
    );
    cx.update(|_, _| {
        assert_eq!(overlap(native_bounds(&page), restored), normal_overlap);
    })
    .unwrap();
    clear_toast(cx).await;
    probe.update(cx, |probe, cx| {
        probe.position = None;
        cx.notify();
    });
    frame(cx).await;
    cx.update(|_, _| {
        assert_eq!(
            native_bounds(&page),
            native,
            "overlay checks must preserve native page geometry"
        )
    })
    .unwrap();
    assert_eq!(
        clicks.get(),
        initial_clicks + 4,
        "tooltip, menu, dialog and toast must each receive one OS click"
    );
    std::fs::write(
        output.join(format!("{name}-overlays.json")),
        serde_json::json!({
            "controlled_frames": true, "native_visible": true, "tooltip": true,
            "menu": true, "dialog": true, "toast": true,
            "os_click_sequences": 5, "action_callbacks": 4,
            "controlled_notification_insets": true, "normal_placement_restored": true,
            "normal_toast_native_overlap": normal_overlap
        })
        .to_string(),
    )
    .unwrap();
}
