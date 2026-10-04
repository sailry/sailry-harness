use gpui_kit::*;

pub(crate) fn baseline() -> Size<Pixels> {
    size(px(760.), px(560.))
}

pub(super) fn minimum(content: Size<Pixels>, chrome: Size<Pixels>) -> Size<Pixels> {
    let floor = baseline();
    size(
        (content.width + chrome.width).max(floor.width),
        (content.height + chrome.height).max(floor.height),
    )
}

/// GPUI exposes minimum size only at window creation. Update AppKit's native
/// constraint at runtime; other platforms use the shared resize fallback.
pub(super) fn apply(mut minimum: Size<Pixels>, window: &mut Window, cx: &mut App) {
    let available = window
        .display(cx)
        .map(|display| display.visible_bounds().size);
    if let Some(available) = available {
        minimum.width = minimum.width.min(available.width);
        minimum.height = minimum.height.min(available.height);
    }
    let previous = window.use_keyed_state("shell-window-minimum", cx, |_, _| None);
    if previous.read(cx).as_ref() != Some(&minimum) {
        previous.update(cx, |previous, _| *previous = Some(minimum));
        #[cfg(target_os = "macos")]
        crate::macos::set_min_size(window, minimum);
    }
    let current = window.viewport_size();
    if current.width < minimum.width || current.height < minimum.height {
        window.defer(cx, move |window, cx| {
            if previous.read(cx).as_ref() != Some(&minimum) {
                return;
            }
            let current = window.viewport_size();
            window.resize(size(
                current.width.max(minimum.width),
                current.height.max(minimum.height),
            ));
        });
    }
}

#[cfg(test)]
mod minimums {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn keeps_application_floor() {
        assert_eq!(
            minimum(size(px(320.), px(0.)), size(px(56.), px(48.))),
            baseline()
        );
    }

    #[test]
    fn adds_content_and_chrome() {
        assert_eq!(
            minimum(size(px(1000.), px(769.)), size(px(336.), px(48.))),
            size(px(1336.), px(817.)),
        );
    }
}
