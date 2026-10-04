//! Shell geometry and surfaces. Features supply optional navigation and details
//! slots; their headers align with the outer band without another header layer.
mod corner;

use super::limits;

use crate::{
    preview::{HEADER_HEIGHT, Layout, Page, RAIL_WIDTH},
    tr,
};
use gpui_kit::base::{Transition, transition};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::{ops::Range, rc::Rc};

pub(crate) struct Geometry {
    pub visible: bool,
    pub panel_width: f32,
    sidebar_open: bool,
    sidebar_width: Pixels,
    navigation_range: Range<Pixels>,
    details_range: Range<Pixels>,
    min_content_width: Pixels,
    compact_width: Option<Entity<Pixels>>,
    window_content_width: Pixels,
    window_content_height: Pixels,
}

impl Geometry {
    pub(crate) fn window_minimum(&self) -> gpui_kit::Size<Pixels> {
        limits::minimum(
            size(
                self.window_content_width
                    + if self.visible {
                        self.details_range.start
                    } else {
                        px(0.)
                    },
                self.window_content_height,
            ),
            size(
                px(RAIL_WIDTH)
                    + if self.sidebar_open {
                        self.navigation_range.start
                    } else {
                        px(0.)
                    },
                px(HEADER_HEIGHT),
            ),
        )
    }
}

impl super::Shell {
    pub(crate) fn frame_geometry(
        &self,
        navigation_available: bool,
        has_panel: bool,
        window: &Window,
        cx: &App,
    ) -> Geometry {
        // Geometry follows the current feature without changing the saved visibility choice.
        let mut layout = Layout {
            sidebar_open: self.layout.sidebar_open && navigation_available,
            ..self.layout.clone()
        };
        let plugin_workspace = self.plugin_workspace(cx);
        let mut constraints = plugin_workspace
            .as_ref()
            .map(|workspace| workspace.read(cx).ranges())
            .unwrap_or_default();
        let compact_width = (self.page == Page::Conversation)
            .then(|| match &self.side_resource {
                Some(crate::resources::SideResource::PreviewFiles(state))
                    if state.tabs.open.is_empty() =>
                {
                    Some(state.list_width.clone())
                }
                Some(crate::resources::SideResource::PreviewGit(state))
                    if state.tabs.open.is_empty() =>
                {
                    Some(state.list_width.clone())
                }
                _ => None,
            })
            .flatten();
        if compact_width.is_some() {
            constraints.details.start = px(crate::resources::split::MIN_WIDTH);
        } else if self.page == Page::Conversation {
            constraints.details = px(crate::resources::MIN_WIDTH)..px(10000.);
        }
        let mut window_content_width = px(self.layout.main_min(self.page));
        let mut window_content_height = px(0.);
        if self.page == Page::Plugin
            && let Some(panel) = self
                .extensions
                .as_ref()
                .and_then(|state| state.panel.as_ref())
        {
            window_content_width = window_content_width.max(panel.read(cx).min_content_width(cx));
            window_content_height = panel.read(cx).min_content_height(cx);
        }
        let sidebar_open = layout.sidebar_open;
        let navigation_width = if sidebar_open {
            px(layout.sidebar_width)
                .clamp(constraints.navigation.start, constraints.navigation.end)
                .min(
                    (window.viewport_size().width - px(RAIL_WIDTH) - window_content_width)
                        .max(constraints.navigation.start),
                )
        } else {
            px(0.)
        };
        // Fit the rendered navigation without overwriting its saved width.
        layout.sidebar_width = f32::from(navigation_width);
        let sidebar_width = px(RAIL_WIDTH) + navigation_width;
        let panel_width = if let Some(workspace) = &plugin_workspace {
            workspace.read(cx).width
        } else if let Some(width) = &compact_width {
            (*width.read(cx))
                .min(window.viewport_size().width - sidebar_width - window_content_width)
        } else {
            px(layout.panel_size(self.page, f32::from(window.viewport_size().width)))
        }
        .clamp(constraints.details.start, constraints.details.end);
        let requested = plugin_workspace
            .as_ref()
            .map_or(layout.panel_open[self.page.panel_index()], |workspace| {
                workspace.read(cx).open
            });
        let panel_minimum = window_content_width
            + px(RAIL_WIDTH)
            + if sidebar_open {
                constraints.navigation.start
            } else {
                px(0.)
            }
            + constraints.details.start;
        let visible = has_panel
            && requested
            && window.viewport_size().width >= sidebar_width + window_content_width + panel_width
            // Release an optional pane at the native minimum so the OS constraint
            // cannot prevent the existing narrow-window hiding behavior.
            && window.viewport_size().width > panel_minimum;
        // A physically smaller display still needs accessible scrolling. The
        // requested width remains authoritative for fit and window constraints.
        let min_content_width =
            window_content_width.min((window.viewport_size().width - sidebar_width).max(px(0.)));
        Geometry {
            visible,
            panel_width: f32::from(panel_width),
            sidebar_open,
            sidebar_width,
            navigation_range: constraints.navigation,
            details_range: constraints.details,
            min_content_width,
            compact_width,
            window_content_width,
            window_content_height,
        }
    }

    pub(super) fn frame(
        &self,
        content: AnyElement,
        mut slots: Option<Slots>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Frame, bool, f32) {
        let navigation = match slots.as_mut() {
            Some(slots) => match slots.navigation {
                crate::plugins::workspace::Navigation::Default => {
                    self.feature_navigation(window, cx)
                }
                crate::plugins::workspace::Navigation::None => None,
                crate::plugins::workspace::Navigation::Resource => slots.sidebar.take(),
            },
            None => self.feature_navigation(window, cx),
        };
        let navigation_available = navigation.is_some();
        let has_panel = slots.as_ref().map_or_else(
            || self.has_resource_panel(cx),
            |slots| slots.details.is_some(),
        );
        let geometry = self.frame_geometry(navigation_available, has_panel, window, cx);
        limits::apply(geometry.window_minimum(), window, cx);
        let Geometry {
            visible,
            panel_width,
            sidebar_open,
            sidebar_width,
            navigation_range,
            details_range,
            min_content_width,
            compact_width,
            ..
        } = geometry;
        let details = match &mut slots {
            Some(slots) => slots.details.take(),
            None => has_panel.then(|| self.details(px(panel_width), window, cx)),
        };
        let plugin_workspace = self.plugin_workspace(cx);
        let panel_index = self.page.panel_index();
        let navigation_resize = navigation_range.clone();
        let details_resize = details_range.clone();
        let frame = Frame {
            state: self.panels.clone(),
            workspace_state: plugin_workspace
                .as_ref()
                .map(|workspace| workspace.read(cx).panels.clone())
                .unwrap_or_else(|| self.workspace_panels.clone()),
            sidebar: navigation.map(|content| Pane {
                visible: sidebar_open,
                width: sidebar_width - px(RAIL_WIDTH),
                range: navigation_range,
                content,
            }),
            rail: self.feature_rail(window, cx).into_any_element(),
            window_header: crate::header::Header::workspace("shell-window-header", cx)
                .pl(if cfg!(target_os = "macos") {
                    px(80.)
                } else {
                    px(12.)
                })
                .children(
                    sidebar_open.then(|| div().flex_1().min_w_0().truncate().child(tr("app"))),
                )
                .into_any_element(),
            module_header: self
                .module_header(
                    navigation_available,
                    visible,
                    window.viewport_size().width
                        - sidebar_width
                        - if visible { px(panel_width) } else { px(0.) },
                    window,
                    cx,
                )
                .into_any_element(),
            content,
            details: details.map(|content| Pane {
                visible,
                width: px(panel_width),
                range: details_range,
                content,
            }),
            min_content_width,
            on_resize: Box::new(
                cx.listener(move |this, state: &Entity<ResizableState>, _, cx| {
                    let sizes = state.read(cx).sizes().to_vec();
                    if sidebar_open {
                        this.layout.sidebar_width = f32::from(
                            (sizes[0] - px(RAIL_WIDTH))
                                .clamp(navigation_resize.start, navigation_resize.end),
                        );
                    }
                }),
            ),
            on_workspace_resize: Box::new(cx.listener(
                move |this, state: &Entity<ResizableState>, _, cx| {
                    let sizes = state.read(cx).sizes().to_vec();
                    if visible && let Some(workspace) = &plugin_workspace {
                        workspace.update(cx, |state, cx| {
                            state.width = sizes[1].clamp(details_resize.start, details_resize.end);
                            cx.notify();
                        });
                    } else if visible && let Some(width) = &compact_width {
                        width.update(cx, |width, cx| {
                            *width = sizes[1].clamp(details_resize.start, details_resize.end);
                            cx.notify();
                        });
                    } else if visible {
                        if panel_index == 0 {
                            this.layout.conversation_panel_resized = true;
                        }
                        this.layout.panel_width[panel_index] =
                            f32::from(sizes[1].clamp(details_resize.start, details_resize.end));
                    }
                },
            )),
        };
        (frame, visible, panel_width)
    }
}

pub struct Pane {
    pub visible: bool,
    pub width: Pixels,
    pub range: Range<Pixels>,
    pub content: AnyElement,
}

pub type ResizeHandler = Box<dyn Fn(&Entity<ResizableState>, &mut Window, &mut App)>;

/// Layout slots retain their feature-owned content and state.
#[derive(IntoElement)]
pub struct Frame {
    pub state: Entity<ResizableState>,
    pub workspace_state: Entity<ResizableState>,
    pub sidebar: Option<Pane>,
    pub rail: AnyElement,
    pub window_header: AnyElement,
    pub module_header: AnyElement,
    pub content: AnyElement,
    pub details: Option<Pane>,
    pub min_content_width: Pixels,
    pub on_resize: ResizeHandler,
    pub on_workspace_resize: ResizeHandler,
}

fn divider(visible: bool) -> gpui_kit::base::ResizeHandleRenderer {
    Rc::new(move |handle, _, cx| {
        if !visible {
            return Some(div().into_any_element());
        }
        let color = if handle.is_active() {
            cx.theme().primary
        } else {
            cx.theme().sidebar_border
        };
        Some(
            div()
                .relative()
                .size_full()
                .child(
                    div()
                        .absolute()
                        .top_3()
                        .left_0()
                        .w(px(1.))
                        .h(px(HEADER_HEIGHT - 24.))
                        .bg(color),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(HEADER_HEIGHT))
                        .bottom_0()
                        .left_0()
                        .w(px(1.))
                        .bg(color),
                )
                .into_any_element(),
        )
    })
}

impl RenderOnce for Frame {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let sidebar_visible = self.sidebar.as_ref().is_some_and(|pane| pane.visible);
        let sidebar_width = self.sidebar.as_ref().map_or(px(0.), |pane| pane.width);
        let openness = if self.sidebar.is_some() {
            transition(
                "sidebar-openness",
                if sidebar_visible { 1.0_f32 } else { 0.0 },
                Transition::new(cx.theme().motion_tokens().duration_normal)
                    .easing(cx.theme().motion_tokens().easing_move.clone()),
                window,
                cx,
            )
        } else {
            0.
        };
        let mounted = sidebar_visible || openness > 0.;
        let left_width = px(RAIL_WIDTH) + sidebar_width * openness;
        let left_range = match self.sidebar.as_ref() {
            _ if openness > 0. && openness < 1. => left_width..left_width,
            Some(pane) if pane.visible => {
                (pane.range.start + px(RAIL_WIDTH))..(pane.range.end + px(RAIL_WIDTH))
            }
            _ => px(RAIL_WIDTH)..px(RAIL_WIDTH),
        };
        let detail_geometry = self.details.as_ref().map(|pane| {
            (
                pane.visible
                    && window.viewport_size().width
                        >= left_width + self.min_content_width + pane.width,
                pane.width,
            )
        });
        let geometry = (
            window.viewport_size(),
            self.min_content_width,
            left_width,
            sidebar_width,
            detail_geometry,
        );
        let previous = window.use_keyed_state("shell-panel-geometry", cx, |_, _| None);
        if previous.read(cx).as_ref() != Some(&geometry) {
            self.state.update(cx, |state, _| state.clear());
            self.workspace_state.update(cx, |state, _| state.clear());
            previous.update(cx, |previous, _| *previous = Some(geometry));
        }
        let workspace = Workspace {
            state: self.workspace_state,
            content: self.content,
            header: self.module_header,
            details: self.details,
            minimum: self.min_content_width,
            left_width,
            sidebar_mounted: mounted,
            on_resize: self.on_workspace_resize,
        }
        .into_any_element();
        let left = v_flex()
            .size_full()
            .min_w_0()
            .child(
                div()
                    .h(px(HEADER_HEIGHT))
                    .flex_shrink_0()
                    .bg(crate::theme::sidebar_background(cx))
                    .child(self.window_header),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .child(
                        div()
                            .w(px(RAIL_WIDTH))
                            .flex_shrink_0()
                            .h_full()
                            .bg(crate::theme::sidebar_background(cx))
                            .child(self.rail),
                    )
                    .overflow_hidden()
                    .when_some(self.sidebar.filter(|_| mounted), |row, pane| {
                        row.child(
                            div()
                                .debug_selector(|| "shell-navigation".into())
                                .relative()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .rounded_tl(cx.theme().radius_lg)
                                .overflow_hidden()
                                .bg(crate::theme::navigation_background(cx))
                                .children(crate::theme::background("background.sidebar", cx))
                                .child(pane.content),
                        )
                    }),
            );
        let panels = h_resizable("shell-panels")
            .with_state(&self.state)
            .with_handle_appearance(divider(mounted))
            .on_resize(self.on_resize)
            .child(
                resizable_panel()
                    .size(left_width)
                    .size_range(left_range)
                    .flex_none()
                    .child(left),
            )
            .child(
                resizable_panel()
                    .size_range(
                        (self.min_content_width
                            + detail_geometry
                                .filter(|(visible, _)| *visible)
                                .map_or(px(0.), |(_, width)| width))
                            ..px(10000.),
                    )
                    .child(
                        div()
                            .debug_selector(|| "main-region".into())
                            .size_full()
                            .min_w_0()
                            .child(workspace),
                    ),
            );
        div()
            .debug_selector(|| "shell-material".into())
            .relative()
            .size_full()
            .child(corner::material(
                cx.theme().radius_lg,
                crate::theme::sidebar_background(cx),
            ))
            .child(panels)
            .child(
                div()
                    .debug_selector(|| "shell-content-outline".into())
                    .absolute()
                    .left(px(RAIL_WIDTH))
                    .top(px(HEADER_HEIGHT))
                    .right_0()
                    .bottom_0()
                    .rounded_tl(cx.theme().radius_lg)
                    .border_t_1()
                    .border_l_1()
                    .border_color(cx.theme().sidebar_border),
            )
    }
}

/// Eager script slots are consumed by the native frame in the same render lifetime.
#[derive(IntoElement)]
pub(crate) struct Plugin {
    pub owner: WeakEntity<super::Shell>,
    pub content: AnyElement,
    pub slots: Slots,
}

pub(crate) struct Slots {
    pub details: Option<AnyElement>,
    pub navigation: crate::plugins::workspace::Navigation,
    pub sidebar: Option<AnyElement>,
}

impl RenderOnce for Plugin {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.owner
            .update(cx, |shell, cx| {
                shell
                    .frame(self.content, Some(self.slots), window, cx)
                    .0
                    .into_any_element()
            })
            .unwrap_or_else(|_| div().into_any_element())
    }
}

fn main(
    content: AnyElement,
    header: AnyElement,
    left_width: Pixels,
    mounted: bool,
    cx: &App,
) -> Div {
    v_flex()
        .debug_selector(|| "main-content".into())
        .size_full()
        .min_w_0()
        .overflow_hidden()
        .child(
            div()
                .flex_shrink_0()
                .bg(crate::theme::sidebar_background(cx))
                .pl(if cfg!(target_os = "macos") {
                    (px(80.) - left_width).max(px(0.))
                } else {
                    px(0.)
                })
                .child(header),
        )
        .child(
            v_flex()
                .debug_selector(|| "shell-body".into())
                .relative()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .when(!mounted, |body| body.rounded_tl(cx.theme().radius_lg))
                .overflow_hidden()
                .bg(crate::theme::panel_background(cx))
                .children(crate::theme::background("background.content", cx))
                .children(crate::theme::banner("banner.app", cx))
                .child(v_flex().flex_1().min_h_0().overflow_hidden().child(content)),
        )
}

#[derive(IntoElement)]
pub(crate) struct Workspace {
    pub state: Entity<ResizableState>,
    pub content: AnyElement,
    pub header: AnyElement,
    pub details: Option<Pane>,
    pub minimum: Pixels,
    pub left_width: Pixels,
    pub sidebar_mounted: bool,
    pub on_resize: ResizeHandler,
}

impl RenderOnce for Workspace {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            state,
            content,
            header,
            details: pane,
            minimum,
            left_width,
            sidebar_mounted,
            on_resize,
        } = self;
        let main = main(content, header, left_width, sidebar_mounted, cx);
        let mut details = resizable_panel().visible(false);
        if let Some(pane) = pane {
            let visible =
                pane.visible && window.viewport_size().width >= left_width + minimum + pane.width;
            details = details
                .visible(visible)
                .size(pane.width)
                .size_range(pane.range)
                .flex_none()
                .child(
                    div()
                        .relative()
                        .size_full()
                        // Tint only the outer band; container opacity must not compound
                        // with a second full-window layer underneath it.
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .right_0()
                                .h(px(HEADER_HEIGHT))
                                .bg(crate::theme::sidebar_background(cx)),
                        )
                        // The existing details header occupies the outer header band.
                        .child(
                            div()
                                .absolute()
                                .top(px(HEADER_HEIGHT))
                                .bottom_0()
                                .left_0()
                                .right_0()
                                .bg(crate::theme::panel_background(cx)),
                        )
                        .child(div().relative().size_full().child(pane.content)),
                );
        }

        h_resizable("workspace-panels")
            .with_state(&state)
            .with_handle_appearance(divider(true))
            .on_resize(on_resize)
            .child(
                resizable_panel()
                    .size_range(minimum..px(10000.))
                    .child(main),
            )
            .child(details)
    }
}
