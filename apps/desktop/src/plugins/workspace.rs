//! Kit's script host has one root and eagerly materialized component children.
//! Compose workspace slots in that root using the Shell's native frame;
//! no child element or second script controller crosses render lifetimes.
use super::host::Host;
use crate::{
    preview::ASSISTANT_WIDTH,
    shell::{Shell, frame},
};
use gpui_kit::{component::ResizableState, *};
use gpui_shell::HostModule;
use std::{ops::Range, sync::Arc};

#[derive(Clone, PartialEq)]
pub(crate) struct Constraints {
    pub(crate) navigation: Range<Pixels>,
    pub(crate) details: Range<Pixels>,
}

impl Default for Constraints {
    fn default() -> Self {
        Self {
            navigation: px(crate::sidebar::WIDTH_RANGE.start)..px(crate::sidebar::WIDTH_RANGE.end),
            details: px(260.)..px(480.),
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
pub(crate) enum Navigation {
    #[default]
    Default,
    None,
    Resource,
}

pub(crate) struct State {
    pub(crate) open: bool,
    pub(crate) width: Pixels,
    pub(crate) panels: Entity<ResizableState>,
    pub(crate) embedded: bool,
    container_width: Pixels,
    controls_width: Entity<Pixels>,
    projection: Option<Projection>,
    owner: Option<WeakEntity<Shell>>,
}

struct Projection {
    host: std::sync::Weak<Host>,
    has_details: bool,
    details_label: Option<SharedString>,
    navigation: Navigation,
    has_navigation: bool,
    constraints: Constraints,
}

impl State {
    pub(super) fn new(cx: &mut Context<Self>) -> Self {
        Self {
            open: true,
            width: px(ASSISTANT_WIDTH),
            panels: cx.new(|_| ResizableState::default()),
            projection: None,
            owner: None,
            embedded: false,
            container_width: px(600.),
            controls_width: cx.new(|_| px(220.)),
        }
    }

    pub(crate) fn embedded(cx: &mut Context<Self>) -> Self {
        let mut state = Self::new(cx);
        state.embedded = true;
        state
    }
    pub(crate) fn resize(&mut self, width: Pixels, cx: &mut Context<Self>) {
        if self.container_width != width {
            self.container_width = width;
            cx.notify();
        }
    }

    fn mounted(&self) -> bool {
        self.projection
            .as_ref()
            .and_then(|projection| projection.host.upgrade())
            .is_some_and(|host| host.check().is_ok())
    }

    pub(crate) fn has_details(&self) -> bool {
        self.mounted()
            && self
                .projection
                .as_ref()
                .is_some_and(|projection| projection.has_details)
    }

    pub(crate) fn details_label(&self) -> Option<SharedString> {
        self.has_details()
            .then(|| self.projection.as_ref()?.details_label.clone())
            .flatten()
    }

    pub(crate) fn ranges(&self) -> Constraints {
        self.projection
            .as_ref()
            .map(|projection| projection.constraints.clone())
            .unwrap_or_default()
    }

    pub(crate) fn has_navigation(&self) -> bool {
        self.projection
            .as_ref()
            .is_some_and(|projection| match projection.navigation {
                Navigation::Default | Navigation::None => false,
                Navigation::Resource => projection.has_navigation,
            })
    }

    pub(crate) fn bind(&mut self, owner: WeakEntity<Shell>, cx: &mut Context<Self>) {
        if self.owner.as_ref() != Some(&owner) {
            self.owner = Some(owner);
            cx.notify();
        }
    }

    fn project(&mut self, next: Projection, cx: &mut Context<Self>) {
        let changed_host = self
            .projection
            .as_ref()
            .is_none_or(|projection| !projection.host.ptr_eq(&next.host));
        if self.projection.as_ref().is_none_or(|projection| {
            !projection.host.ptr_eq(&next.host)
                || projection.has_details != next.has_details
                || projection.details_label != next.details_label
                || projection.navigation != next.navigation
                || projection.has_navigation != next.has_navigation
                || projection.constraints != next.constraints
        }) {
            if changed_host {
                self.owner = None;
            }
            self.projection = Some(next);
            cx.notify();
        }
    }
}

pub(super) fn module(
    module: HostModule,
    workspace: Option<Entity<State>>,
    host: Arc<Host>,
) -> HostModule {
    let mode = workspace.clone();
    let mode_host = host.clone();
    let declarations = format!(
        "{}\nexport function workspaceMode(): \"main\" | \"embedded\";\n/** Children are content, optional details, and optional resource navigation; Shell owns the frame. Navigation/details minimum widths are bounded by 320/480px; main content uses Header.min_content_width. */\nexport const Workspace: {{ new(id: string, props?: {{has_details?: boolean; details_label?: string; default_details_width?: number; min_navigation_width?: number; min_details_width?: number; navigation?: \"default\" | \"none\" | \"resource\"}}): import(\"gpui-kit\").Element }};",
        module.declared().unwrap_or_default()
    );
    module
        .function("workspaceMode", move |_| {
            mode_host.check()?;
            gpui_shell::with_current_app(|cx| {
                Ok(gpui_shell::HostValue::from(
                    if mode.as_ref().is_some_and(|state| state.read(cx).embedded) {
                        "embedded"
                    } else {
                        "main"
                    },
                ))
            })
            .ok_or_else(|| gpui_shell::HostError::new("workspace is unavailable"))?
        })
        .component("Workspace", move |mut args, _, cx| {
            if host.check().is_err() {
                return div().into_any_element();
            }
            let available = args
                .props()
                .get("has_details")
                .and_then(|value| value.as_bool())
                != Some(false);
            let details_label = args
                .props()
                .get("details_label")
                .and_then(|value| value.as_str())
                .filter(|label| !label.is_empty())
                .map(|label| SharedString::from(label.to_owned()));
            let default_details_width = args
                .props()
                .get("default_details_width")
                .and_then(|value| value.as_number())
                .filter(|width| width.is_finite() && (260. ..=480.).contains(width))
                .map(|width| px(width as f32));
            let navigation = match args
                .props()
                .get("navigation")
                .and_then(|value| value.as_str())
            {
                None | Some("default") => Navigation::Default,
                Some("none") => Navigation::None,
                Some("resource") => Navigation::Resource,
                Some(_) => return div().into_any_element(),
            };
            let mut constraints = Constraints::default();
            for (name, range) in [
                ("min_navigation_width", &mut constraints.navigation),
                ("min_details_width", &mut constraints.details),
            ] {
                if let Some(width) = args.props().get(name).and_then(|value| value.as_number())
                    && width.is_finite()
                    && (0. ..=f32::from(range.end) as f64).contains(&width)
                {
                    range.start = px(width as f32);
                }
            }
            let mut children = args.take_children().into_iter();
            let content = children.next().unwrap_or_else(|| div().into_any_element());
            let details = children.next();
            let sidebar = children.next();
            let Some(workspace) = &workspace else {
                return content;
            };
            workspace.update(cx, |state, cx| {
                // A default applies only on first projection; native divider
                // changes and reloads keep the same pane's user-owned width.
                if state.projection.is_none()
                    && let Some(width) = default_details_width
                {
                    state.width = width;
                }
                state.project(
                    Projection {
                        host: Arc::downgrade(&host),
                        has_details: available && details.is_some(),
                        details_label,
                        navigation,
                        has_navigation: sidebar.is_some(),
                        constraints,
                    },
                    cx,
                );
            });
            let state = workspace.read(cx);
            if state.embedded {
                return crate::resources::split::Split {
                    id: format!("plugin-resource-split-{}", workspace.entity_id()).into(),
                    width: state.container_width,
                    content,
                    controls: details.unwrap_or_else(|| div().into_any_element()),
                    controls_width: Some(state.controls_width.clone()),
                }
                .into_any_element();
            }
            let Some(owner) = state.owner.clone() else {
                // The Shell supplies its ordinary frame until this projection is mounted.
                return content;
            };
            frame::Plugin {
                owner,
                content,
                slots: frame::Slots {
                    details: available.then_some(details).flatten(),
                    navigation,
                    sidebar,
                },
            }
            .into_any_element()
        })
        .declarations(declarations)
}

impl Shell {
    pub(crate) fn plugin_workspace(&self, cx: &App) -> Option<Entity<State>> {
        if self.page != crate::preview::Page::Plugin || self.needs_project() {
            return None;
        }
        self.extensions
            .as_ref()?
            .panel
            .as_ref()?
            .read(cx)
            .workspace
            .clone()
            .filter(|state| {
                let state = state.read(cx);
                !state.embedded && state.mounted()
            })
    }

    pub(crate) fn plugin_worktree(&self) -> bool {
        self.page == crate::preview::Page::Plugin
            && self
                .extensions
                .as_ref()
                .and_then(|state| state.selected.as_ref())
                .is_some_and(|entry| {
                    entry.navigation.target
                        == sailry_protocol::plugin::desktop::NavigationTarget::Worktree
                })
    }
}
