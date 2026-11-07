//! One composer popover for model choice and discrete reasoning levels.
use super::*;
use crate::model_picker::Picker;
use gpui_kit::base::{Transition, transition};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    popover::Popover,
    slider::{SliderEvent, SliderState},
    tag::Tag,
    tooltip::Tooltip,
};
use gpui_kit::prelude::FluentBuilder as _;

mod badge;
pub(super) mod features;
mod fit;
pub(super) mod team;
#[cfg(test)]
mod tests;
mod track;

/// Width caps for the composer model chip.
const CHIP_WIDTH: f32 = 260.;
const COMPACT_CHIP_WIDTH: f32 = 220.;
const NARROW_CHIP_WIDTH: f32 = 140.;

impl View {
    fn model_label(&self) -> SharedString {
        let Some(config) = &self.config else {
            return tr("composer_select_model");
        };
        self.providers()
            .find(|provider| provider.id == config.provider)
            .and_then(|provider| {
                provider
                    .models
                    .iter()
                    .find(|model| model.id == config.model)
            })
            .and_then(|model| model.display_name.clone())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| config.model.clone())
            .into()
    }

    pub(super) fn sync_model_controls(&self, cx: &mut Context<Self>) -> bool {
        let roster = self.team_roster();
        // A member that left the roster hands the controls back to the lead.
        let member = self
            .model_controls
            .read(cx)
            .member
            .filter(|role| roster.contains(*role));
        let edited = member.map(|role| self.member_settings(role));
        let target = self.binding.client.target();
        let selected = match &edited {
            Some(edited) => edited.selection.clone(),
            None => self.config.as_ref().and_then(|config| {
                Some(super::super::models::Selection {
                    channel: *self
                        .provider_ids
                        .get(&(self.config_owner, config.provider))?,
                    model: config.model.clone(),
                })
            }),
        };
        let channels = self
            .model_sources()
            // Member models use the execution Node's providers.
            .filter(|(node, _)| member.is_none() || *node == target)
            .filter_map(|(node, provider)| {
                let id = self.provider_ids.get(&(node, provider.id))?;
                let mut channel = crate::settings::channel(*id, provider);
                if node == self.binding.client.target() {
                    channel.directory = crate::settings::installed_preset(
                        provider.preset.as_ref(),
                        &self.contribution_metadata.read(cx).entries,
                    )
                    .or(channel.directory);
                }
                channel.enabled &= self.source_connected(node);
                if let Some(profile) = self
                    .session
                    .as_ref()
                    .and_then(|session| session.profile.as_ref())
                {
                    channel.name = rust_i18n::t!(
                        if profile.provider.id == provider.id {
                            "chat_provider_session"
                        } else {
                            "chat_provider_node"
                        },
                        name = provider.name,
                    )
                    .to_string();
                } else if self.session.is_none()
                    && self.binding.client.target() != self.binding.defaults.target()
                {
                    let source = if node == self.binding.defaults.target() {
                        tr("chat_config_desktop")
                    } else {
                        self.binding.host.clone()
                    };
                    channel.name = rust_i18n::t!(
                        "chat_provider_source",
                        name = provider.name,
                        source = source
                    )
                    .to_string();
                }
                Some(channel)
            })
            .collect::<Vec<_>>();
        let blocked = self.configuration_blocked();
        let available = channels
            .iter()
            .any(|channel| channel.enabled && !channel.models.is_empty());
        let selected_features = self
            .config
            .as_ref()
            .map(|config| config.features.clone())
            .unwrap_or_default();
        let settings = match edited {
            Some(edited) => Settings {
                model: edited.label,
                choices: edited.choices,
                effort: edited.effort,
                features: edited.features,
                selected: edited.selected,
                resettable: edited.overridden,
                ..self.base_settings(roster, member, blocked)
            },
            None => {
                let effort = self
                    .config
                    .as_ref()
                    .map_or(Effort::Default, |config| config.effort);
                Settings {
                    model: self.model_label(),
                    choices: self.available_efforts(),
                    effort,
                    features: self.available_features(),
                    resettable: effort != self.default_effort() || !selected_features.is_empty(),
                    selected: selected_features,
                    ..self.base_settings(roster, None, blocked)
                }
            }
        };
        let effort = settings.effort;
        self.model_picker.update(cx, |picker, cx| {
            picker.set(channels, selected, cx);
            picker.popover = None;
            picker.composer(effort, blocked || !self.can_pick_model(), cx);
        });
        let panel = self.model_controls.clone();
        panel.update(cx, |panel, cx| panel.sync(settings, cx));
        available
    }

    /// Settings shared by every avatar; the caller fills in the edited model.
    fn base_settings(
        &self,
        roster: team::Roster,
        member: Option<sailry_protocol::RoleId>,
        blocked: bool,
    ) -> Settings {
        Settings {
            model: SharedString::default(),
            choices: Vec::new(),
            effort: Effort::Default,
            features: Vec::new(),
            selected: Default::default(),
            resettable: false,
            roster,
            member,
            busy: blocked || !self.can_pick_model(),
            pending: self.pending,
            connected: self.configuration().connected,
        }
    }

    pub(super) fn model_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.sync_model_controls(cx) {
            return self.model_controls.clone().into_any_element();
        }
        Button::new("composer-select-model")
            .ghost()
            .w_full()
            .label(tr("composer_select_model"))
            .disabled(self.configuration_blocked() || !self.can_pick_model())
            .on_click(cx.listener(|view, _, _, cx| cx.emit(Event::Settings(view.settings_node()))))
            .into_any_element()
    }

    pub(super) fn model_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let available = self.sync_model_controls(cx);
        let model = self.model_label();
        let roster = self.team_roster();
        let named = match roster.chip() {
            team::Chip::Model => None,
            team::Chip::Team { team, overridden } => Some((team.clone(), overridden)),
        };
        let provider = self.config.as_ref().and_then(|config| {
            let name = self
                .providers()
                .find(|provider| provider.id == config.provider)?
                .name
                .clone();
            let badge = badge::resolve(
                &name,
                self.model_sources()
                    .map(|(_, provider)| provider.name.as_str()),
            );
            (!badge.is_empty()).then(|| (SharedString::from(badge), SharedString::from(name)))
        });
        let fit = self.model_controls.read(cx).fit.clone();
        let view = cx.entity_id();
        let cap = px(if self.icon_context {
            NARROW_CHIP_WIDTH
        } else if self.sidebar || self.compact_composer {
            COMPACT_CHIP_WIDTH
        } else {
            CHIP_WIDTH
        });
        let compact = self.icon_context || fit.compact(cap);
        let effort = self
            .config
            .as_ref()
            .map_or(Effort::Default, |config| config.effort);
        let choices = self.available_efforts();
        // A team chip names the team; the lead's model and effort move to its tooltip.
        let lead = if choices.is_empty() {
            model.to_string()
        } else {
            format!("{} · {}", model, crate::reasoning::label(effort))
        };
        if named.is_some() {
            fit.forget_effort();
        }
        let segments = self
            .contributions
            .read(cx)
            .entries(sailry_protocol::plugin::ui::Slot::Model, cx)
            .into_iter()
            .flat_map(|entry| entry.segments(cx))
            .collect::<Vec<_>>();
        let (fast_badges, feature_badges): (Vec<_>, Vec<_>) = features::selected(
            &self.available_features(),
            &self
                .config
                .as_ref()
                .map(|config| config.features.clone())
                .unwrap_or_default(),
        )
        .into_iter()
        .filter(|choice| !self.icon_context || choice.icon.is_some())
        .partition(|choice| choice.fast);
        let mut accessibility_label = match &named {
            Some((team, _)) => format!(
                "{}: {} · {}",
                tr("composer_model_settings"),
                team.name,
                lead
            ),
            None => format!("{}: {}", tr("composer_model_settings"), model),
        };
        for choice in fast_badges.iter().chain(&feature_badges) {
            accessibility_label.push_str(" · ");
            accessibility_label.push_str(choice.label.as_ref());
        }
        let team_avatar = named
            .as_ref()
            .map(|(team, overridden)| team::chip_avatar(Some(team), *overridden, cx));
        let badge = |choice: features::Choice| {
            let text = choice.icon.is_none();
            let label = choice.label;
            let tip = label.clone();
            div()
                .id(SharedString::from(format!(
                    "model-feature-summary-{}",
                    choice.id
                )))
                .flex_shrink_0()
                .text_color(cx.theme().primary)
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                .children(
                    choice
                        .icon
                        .map(|icon| icon.size_4().text_color(cx.theme().foreground)),
                )
                .when(text, |badge| badge.child(label))
        };
        let button = Button::new("live-chat-model")
            .custom(crate::theme::subtle_button(cx))
            .rounded_full()
            .max_w(cap)
            .min_w_0()
            .flex_shrink_1()
            .debug_selector(|| "live-chat-model".into())
            .accessibility_label(accessibility_label)
            .when(named.is_some(), |button| button.tooltip(lead.clone()))
            .child(
                h_flex()
                    .min_w_0()
                    .gap_2()
                    .text_sm()
                    .line_height(px(20.))
                    .when_some(
                        provider.filter(|_| !self.icon_context && named.is_none()),
                        |row, (badge, name)| {
                            row.child(
                                div()
                                    .id("live-chat-channel")
                                    .flex_shrink_0()
                                    .tooltip(move |window, cx| {
                                        Tooltip::new(name.clone()).build(window, cx)
                                    })
                                    .child(
                                        Tag::primary()
                                            .small()
                                            .text_xs()
                                            .rounded_full()
                                            .border_0()
                                            .bg(cx.theme().primary.opacity(0.12))
                                            .text_color(cx.theme().primary)
                                            .child(badge),
                                    ),
                            )
                        },
                    )
                    .children(fast_badges.into_iter().map(&badge))
                    .map(|row| match &named {
                        Some((team, _)) => row
                            .child(div().flex_shrink_0().children(team_avatar))
                            .child(
                                div()
                                    .relative()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis_middle()
                                    .child(team.name.clone())
                                    .child(fit.probe(
                                        fit::Part::Model,
                                        Some(team.name.clone()),
                                        view,
                                    )),
                            ),
                        None => row
                            .child(
                                div()
                                    .relative()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis_middle()
                                    .child(model.clone())
                                    .child(fit.probe(fit::Part::Model, Some(model.clone()), view)),
                            )
                            .when(!choices.is_empty(), |row| {
                                let label = crate::reasoning::label(effort);
                                let tip = label.clone();
                                row.child(
                                    div()
                                        .id("live-chat-effort")
                                        .debug_selector(|| "live-chat-effort".into())
                                        .relative()
                                        .flex_shrink_0()
                                        .text_color(cx.theme().muted_foreground)
                                        .map(|effort| {
                                            if compact {
                                                effort
                                                    .child(
                                                        Icon::new(
                                                            gpui_kit::assets::IconName::Brain,
                                                        )
                                                        .size_4(),
                                                    )
                                                    .tooltip(move |window, cx| {
                                                        Tooltip::new(tip.clone()).build(window, cx)
                                                    })
                                            } else {
                                                effort.child(label.clone())
                                            }
                                        })
                                        .child(fit.probe(fit::Part::Effort, Some(label), view)),
                                )
                            }),
                    })
                    .children(feature_badges.into_iter().map(badge))
                    .children(segments.into_iter().filter(|_| !self.icon_context)),
            )
            .disabled(self.configuration_blocked() || !self.can_pick_model());
        let chip = div().relative().min_w_0().flex_shrink_1().child(fit.probe(
            fit::Part::Chip,
            None,
            view,
        ));
        if !available {
            return chip
                .child(button.on_click(
                    cx.listener(|view, _, _, cx| cx.emit(Event::Settings(view.settings_node()))),
                ))
                .into_any_element();
        }
        let panel = self.model_controls.clone();
        let opening = panel.clone();
        let popover = crate::theme::popover(Popover::new("live-model-popover"), cx)
            .open(panel.read(cx).opened)
            .anchor(Anchor::BottomRight)
            .bottom_2()
            .p_3()
            .trigger(button)
            .on_open_change(move |open, window, cx| {
                opening.update(cx, |panel, cx| panel.open(*open, window, cx));
            })
            .content(move |_, _, _| panel.clone());
        chip.child(popover).into_any_element()
    }

    /// Toolbar spacer that reports free room the model chip may grow into.
    pub(super) fn model_spacer(&self, cx: &mut Context<Self>) -> AnyElement {
        let fit = self.model_controls.read(cx).fit.clone();
        div()
            .relative()
            .flex_1()
            .child(fit.probe(fit::Part::Spare, None, cx.entity_id()))
            .into_any_element()
    }
}

/// Controls for the selected avatar: the lead's configuration or one member's model.
#[derive(Clone, PartialEq)]
struct Settings {
    model: SharedString,
    choices: Vec<Effort>,
    effort: Effort,
    features: Vec<sailry_protocol::conversation::features::Feature>,
    selected: std::collections::BTreeMap<String, String>,
    resettable: bool,
    roster: team::Roster,
    /// The edited member; None edits the lead.
    member: Option<sailry_protocol::RoleId>,
    busy: bool,
    /// A command is in flight; team and member changes wait for it.
    pending: bool,
    connected: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Layer {
    Main,
    Models,
    Teams,
}

pub(super) struct Panel {
    pub(super) opened: bool,
    owner: WeakEntity<View>,
    picker: Entity<Picker>,
    contributions: Entity<crate::plugins::contributions::Registry>,
    _contributions: Subscription,
    slider: Entity<SliderState>,
    focus: FocusHandle,
    settings: Option<Settings>,
    layer: Layer,
    /// The selected avatar; None is the lead.
    member: Option<sailry_protocol::RoleId>,
    dragging: bool,
    fit: std::rc::Rc<fit::Fit>,
}

impl Panel {
    pub(super) fn new(
        owner: WeakEntity<View>,
        picker: Entity<Picker>,
        contributions: Entity<crate::plugins::contributions::Registry>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let slider = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(1.)
                .step(1.)
                .default_value(0.)
        });
        cx.subscribe_in(&slider, window, |panel, _, event, window, cx| match event {
            SliderEvent::Change(_) => {
                panel.dragging = true;
                cx.notify();
            }
            SliderEvent::Release(value) => {
                panel.dragging = false;
                panel.apply(value.end() as usize, window, cx);
            }
        })
        .detach();
        cx.subscribe_in(
            &picker,
            window,
            |panel, _, _: &crate::model_picker::Back, window, cx| {
                panel.back(window, cx);
            },
        )
        .detach();
        let observer = cx.observe(&contributions, |_, _, cx| cx.notify());
        Self {
            opened: false,
            contributions,
            _contributions: observer,
            owner,
            picker,
            slider,
            focus: cx.focus_handle(),
            settings: None,
            layer: Layer::Main,
            member: None,
            dragging: false,
            fit: Default::default(),
        }
    }

    fn sync(&mut self, mut settings: Settings, cx: &mut Context<Self>) {
        settings.choices.sort_by_key(|choice| order(*choice));
        if self.member != settings.member {
            self.member = settings.member;
            self.dragging = false;
            cx.notify();
        }
        if self.settings.as_ref() != Some(&settings) {
            self.settings = Some(settings);
            cx.notify();
        }
    }

    pub(super) fn member(&self) -> Option<sailry_protocol::RoleId> {
        self.member
    }

    pub(super) fn request_open(&mut self, cx: &mut Context<Self>) {
        self.opened = true;
        self.layer = Layer::Main;
        self.dragging = false;
        self.member = None;
        cx.notify();
    }

    /// Each opening starts on the first layer with the lead selected.
    pub(super) fn open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.opened = open;
        self.layer = Layer::Main;
        self.dragging = false;
        if open {
            self.focus.focus(window, cx);
            self.member = None;
        }
        let owner = self.owner.clone();
        cx.defer(move |cx| {
            let _ = owner.update(cx, |_, cx| cx.notify());
        });
        cx.notify();
    }

    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.layer = Layer::Main;
        self.dragging = false;
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn apply(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(settings) = &self.settings else {
            return;
        };
        if settings.busy || !settings.connected {
            return;
        }
        let Some(effort) = settings.choices.get(index).copied() else {
            return;
        };
        let member = settings.member;
        let _ = self.owner.update(cx, |view, cx| match member {
            Some(role) => view.select_member_effort(role, effort, window, cx),
            None => view.select_effort(effort, window, cx),
        });
        cx.notify();
    }

    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(settings) = &self.settings else {
            return;
        };
        if self.layer != Layer::Main
            || settings.busy
            || !settings.connected
            || settings.choices.len() < 2
        {
            return;
        }
        let current = self.slider.read(cx).value().end() as usize;
        let last = settings.choices.len() - 1;
        let index = match event.keystroke.key.as_str() {
            "left" | "down" => current.saturating_sub(1),
            "right" | "up" => (current + 1).min(last),
            "home" => 0,
            "end" => last,
            _ => return,
        };
        cx.stop_propagation();
        self.slider
            .update(cx, |slider, cx| slider.set_value(index as f32, window, cx));
        self.apply(index, window, cx);
    }
}

impl Render for Panel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(settings) = self.settings.clone() else {
            return div().into_any_element();
        };
        let size = crate::model_picker::panel_size(window);
        let height = transition(
            "model-controls-height",
            match self.layer {
                Layer::Main => px(104. + team::ROW_HEIGHT),
                Layer::Models | Layer::Teams => size.height,
            },
            Transition::new(cx.theme().motion_tokens().duration_normal)
                .easing(cx.theme().motion_tokens().easing_move.clone()),
            window,
            cx,
        );
        let body = v_flex()
            .id("model-controls")
            .debug_selector(|| "model-controls".into())
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key))
            .w(size.width)
            .h(height)
            .overflow_hidden()
            .items_center()
            .text_sm()
            .gap_2();
        match self.layer {
            Layer::Main => {}
            Layer::Models => return body.child(self.picker.clone()).into_any_element(),
            Layer::Teams => {
                return body
                    .child(self.team_list(&settings, size, cx))
                    .into_any_element();
            }
        }
        let current = settings
            .choices
            .iter()
            .position(|effort| *effort == settings.effort)
            .unwrap_or(0);
        let max = settings.choices.len().saturating_sub(1).max(1) as f32;
        if self.slider.read(cx).max_value() != max {
            self.dragging = false;
            self.slider.update(cx, |slider, _| {
                *slider = SliderState::new()
                    .min(0.)
                    .max(max)
                    .step(1.)
                    .default_value(current as f32)
            });
        }
        if !self.dragging && !settings.busy && self.slider.read(cx).value().end() != current as f32
        {
            self.slider.update(cx, |slider, cx| {
                slider.set_value(current as f32, window, cx)
            });
        }
        let index = self.slider.read(cx).value().end() as usize;
        let selected = settings
            .choices
            .get(index)
            .copied()
            .unwrap_or(settings.effort);
        let highest = settings.choices.len() > 1
            && index + 1 == settings.choices.len()
            && !matches!(
                selected,
                Effort::Default | Effort::Disabled | Effort::Budget(-1)
            );
        let color = if highest {
            cx.theme().warning
        } else {
            cx.theme().primary
        };
        let contributions = self.contributions.update(cx, |registry, cx| {
            registry
                .entries(sailry_protocol::plugin::ui::Slot::Model, cx)
                .into_iter()
                .map(|entry| {
                    registry.control(entry, crate::plugins::contributions::Form::Model, cx)
                })
                .collect::<Vec<_>>()
        });
        let member = settings.member;
        let locked = settings.busy || !settings.connected || (member.is_some() && settings.pending);
        let owner = self.owner.clone();
        let reset = Button::new("model-controls-reset")
            .rounded_full()
            .ghost()
            .small()
            .size_6()
            .icon(gpui_kit::assets::IconName::RotateCcw)
            .accessibility_label(tr("composer_effort_reset"))
            .tooltip(tr("composer_effort_reset"))
            .debug_selector(|| "model-controls-reset".into())
            .disabled(locked || !settings.resettable)
            .on_click(move |_, window, cx| {
                let _ = owner.update(cx, |view, cx| match member {
                    Some(role) => view.select_member_model(role, None, window, cx),
                    None => view.reset_model_settings(window, cx),
                });
            });
        let row = self.team_row(
            &settings,
            settings.busy || settings.pending || !settings.connected,
            cx,
        );
        let controls = v_flex()
            .w_full()
            .flex_shrink_0()
            .gap_2()
            .child(
                v_flex()
                    .w_full()
                    .min_w_0()
                    .items_center()
                    .gap_1()
                    .child(
                        h_flex()
                            .relative()
                            .w_full()
                            .h_6()
                            .justify_center()
                            .child(
                                h_flex()
                                    .absolute()
                                    .left_0()
                                    .top_0()
                                    .h_6()
                                    .items_center()
                                    .gap_1()
                                    .when(
                                        member.is_none()
                                            && !settings
                                                .features
                                                .iter()
                                                .any(|feature| feature.id == "speed"),
                                        |row| row.child(features::unsupported()),
                                    )
                                    .children(settings.features.iter().map(|feature| {
                                        features::control(
                                            feature,
                                            settings.selected.get(&feature.id).map(String::as_str),
                                            locked,
                                            member,
                                            self.owner.clone(),
                                        )
                                    })),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(color)
                                    .debug_selector(|| "model-controls-effort".into())
                                    .child(if settings.choices.is_empty() {
                                        tr("composer_effort_unavailable")
                                    } else {
                                        crate::reasoning::label(selected)
                                    }),
                            )
                            .child(
                                h_flex()
                                    .absolute()
                                    .right_0()
                                    .top_0()
                                    .h_6()
                                    .items_center()
                                    .gap_1()
                                    .when(member.is_none(), |row| row.children(contributions))
                                    .child(reset),
                            ),
                    )
                    .child(
                        Button::new("model-controls-choose")
                            .rounded_full()
                            .text()
                            .small()
                            .h_7()
                            .max_w_full()
                            .accessibility_label(settings.model.clone())
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_sm()
                                    .line_height(px(20.))
                                    .child(settings.model),
                            )
                            .child(Icon::new(IconName::ChevronRight).size_3())
                            .disabled(settings.busy || (member.is_some() && settings.pending))
                            .debug_selector(|| "model-controls-choose".into())
                            .on_click(cx.listener(|panel, _, _, cx| {
                                panel.layer = Layer::Models;
                                panel.picker.update(cx, |picker, cx| picker.reveal(cx));
                                cx.notify();
                            })),
                    ),
            )
            .child(track::render(
                &self.slider,
                &self.focus,
                settings.choices.len(),
                highest,
                locked || settings.choices.len() < 2,
                cx,
            ));
        body.child(row).child(controls).into_any_element()
    }
}

fn order(effort: Effort) -> (u8, i32) {
    match effort {
        Effort::Default | Effort::Budget(-1) => (0, -1),
        Effort::Disabled => (0, 0),
        Effort::Minimal => (1, 0),
        Effort::Low => (2, 0),
        Effort::Medium => (3, 0),
        Effort::High => (4, 0),
        Effort::XHigh => (5, 0),
        Effort::Max => (6, 0),
        Effort::Budget(value) => (7, value),
    }
}
