//! Kit's script Scroll omits nested wheel masks and cannot export its opaque handle.
//! Compose the public native area, mask and scrollbar without a parallel scroll control.
use gpui_kit::{
    component::{scroll::ScrollableMask, *},
    *,
};
use gpui_shell::{HostModule, HostValue};

#[cfg(test)]
mod tests;

pub(super) fn extend(module: HostModule) -> HostModule {
    let declarations = format!(
        "{}\n{}",
        module.declared().unwrap_or_default(),
        "export const ScrollRegion: { new(id: string, props: { max_height: number }): import('gpui-kit').Element };"
    );
    module
        .component("ScrollRegion", |mut args, window, cx| {
            let id: SharedString = args.id().to_owned().into();
            let height = args
                .props()
                .get("max_height")
                .and_then(HostValue::as_number)
                .filter(|value| value.is_finite() && *value > 0. && *value <= f32::MAX as f64)
                .unwrap_or(520.) as f32;
            let handle = window
                .use_keyed_state(id.clone(), cx, |_, _| ScrollHandle::default())
                .read(cx)
                .clone();
            div()
                .id(id.clone())
                .relative()
                .w_full()
                .min_w_0()
                .child(
                    div()
                        .id((ElementId::from(id.clone()), "area"))
                        .w_full()
                        .h_auto()
                        .min_h_0()
                        .max_h(px(height))
                        .flex()
                        .flex_col()
                        .gap_3()
                        .overflow_y_scroll()
                        .track_scroll(&handle)
                        .lock_scroll_axis()
                        .children(args.take_children()),
                )
                .child(ScrollableMask::new(Axis::Vertical, &handle).id(id.clone()))
                .child(
                    div().absolute().inset_0().child(
                        scroll::Scrollbar::new(&handle)
                            .axis(scroll::ScrollbarAxis::Vertical)
                            .viewport_from_layout(),
                    ),
                )
                .into_any_element()
        })
        .declarations(declarations)
}
