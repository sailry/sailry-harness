//! Composer commands follow Code 67ae9fa0's catalog and dispatch to existing UI actions.
use super::*;
use crate::conversation::models::Selection;

#[derive(Clone)]
pub(super) enum Choice {
    Model(Selection, Effort, String),
    Reasoning(Effort),
    Prompt(&'static str),
    Mode(sailry_protocol::WorkMode),
    External(Box<crate::plugins::contributions::Entry>),
    Compact,
    Settings,
    Skill(
        sailry_protocol::plugin::Reference,
        Box<sailry_protocol::plugin::Skill>,
    ),
}

impl Choice {
    pub fn label(&self) -> SharedString {
        match self {
            Self::Model(model, ..) => model.model.clone().into(),
            Self::Reasoning(effort) => crate::reasoning::label(*effort),
            Self::Prompt(name) => format!("/{name}").into(),
            Self::Mode(mode) => mode_token(*mode).into(),
            Self::External(entry) => {
                format!("/{}", entry.declaration.command.as_ref().unwrap().name).into()
            }
            Self::Compact => "/compact".into(),
            Self::Settings => "/model".into(),
            Self::Skill(_, skill) => crate::plugins::metadata::skill_title(skill).into(),
        }
    }

    pub fn icon(&self) -> IconName {
        match self {
            Self::Model(..) | Self::Settings => IconName::Bot,
            Self::Reasoning(_) => IconName::Bot,
            Self::Skill(..) => IconName::BookOpen,
            Self::Compact => IconName::Minimize,
            Self::Prompt("review") => IconName::Eye,
            Self::Prompt("test") => IconName::SquareTerminal,
            Self::Prompt("plan") => IconName::LayoutDashboard,
            Self::Mode(_) => IconName::LayoutDashboard,
            Self::External(_) => IconName::Info,
            Self::Prompt(_) => IconName::Info,
        }
    }
}

impl Item {
    pub fn description(&self) -> SharedString {
        match self {
            Self::Current(reference) => reference.label.clone().into(),
            Self::Plugin(info) => crate::plugins::metadata::description(info)
                .unwrap_or_default()
                .into(),
            Self::Page(Page::Models) | Self::Command(Choice::Settings) => {
                tr("composer_select_model")
            }
            Self::Page(Page::Reasoning) => tr("composer_effort"),
            Self::Command(Choice::Model(_, _, provider)) => provider.clone().into(),
            Self::Command(Choice::Compact) => tr("chat_compact"),
            Self::Command(Choice::Prompt(name)) => tr(&format!("composer_command_{name}")),
            Self::Command(Choice::Mode(mode)) => tr(crate::conversation::mode::label(*mode)),
            Self::Command(Choice::External(entry)) => entry.text(),
            Self::Command(Choice::Skill(_, skill)) => skill.description.clone().into(),
            _ => "".into(),
        }
    }
}

impl View {
    pub(super) fn reference_root(&self) -> Page {
        if self
            .references
            .trigger
            .as_ref()
            .is_some_and(|trigger| trigger.text.as_bytes()[trigger.range.start] == b'/')
        {
            Page::Commands
        } else if matches!(self.composer_options.mentions, Mentions::Database)
            && let Some(profile) = self.reference_databases().first()
        {
            Page::Database(profile.id, profile.name.clone(), None)
        } else {
            Page::Root
        }
    }

    pub(super) fn command_catalog(&self, page: &Page, cx: &App) -> Vec<Item> {
        match page {
            Page::Models => self
                .model_sources()
                .filter(|(node, provider)| provider.enabled && self.source_connected(*node))
                .flat_map(|(node, provider)| {
                    let channel = self.provider_ids.get(&(node, provider.id)).copied();
                    provider.models.iter().filter_map(move |model| {
                        Some(Item::Command(Choice::Model(
                            Selection {
                                channel: channel?,
                                model: model.id.clone(),
                            },
                            Effort::initial(&model.efforts, model.default_effort),
                            provider.name.clone(),
                        )))
                    })
                })
                .collect(),
            Page::Reasoning => self
                .available_efforts()
                .into_iter()
                .map(|effort| Item::Command(Choice::Reasoning(effort)))
                .collect(),
            Page::Commands => {
                let mut rows = vec![if self.command_catalog(&Page::Models, cx).is_empty() {
                    Item::Command(Choice::Settings)
                } else {
                    Item::Page(Page::Models)
                }];
                if !self.available_efforts().is_empty() {
                    rows.push(Item::Page(Page::Reasoning));
                }
                if self.can_compact() {
                    rows.push(Item::Command(Choice::Compact));
                }
                if !self.readonly() {
                    rows.extend(
                        [
                            sailry_protocol::WorkMode::Plan,
                            sailry_protocol::WorkMode::Code,
                        ]
                        .into_iter()
                        .map(|mode| Item::Command(Choice::Mode(mode))),
                    );
                }
                rows.extend(
                    self.composer_options
                        .prompts
                        .iter()
                        .filter(|name| **name != "plan")
                        .map(|name| Item::Command(Choice::Prompt(name))),
                );
                rows.extend(
                    self.contributions
                        .read(cx)
                        .commands(cx)
                        .into_iter()
                        .map(|entry| Item::Command(Choice::External(Box::new(entry)))),
                );
                rows
            }
            _ => vec![],
        }
    }

    pub(super) fn choose_command(
        &mut self,
        choice: Choice,
        trigger: Trigger,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Choice::External(entry) = &choice
            && !self
                .contributions
                .read(cx)
                .commands(cx)
                .iter()
                .any(|current| current.key == entry.key && current.state.enabled)
        {
            self.error = Some("reference_stale");
            self.reference_page(Page::Commands, window, cx);
            return;
        }
        if let Choice::Skill(package, _) = &choice
            && !self.skill_packages().contains(package)
        {
            self.error = Some("reference_stale");
            self.reference_page(Page::Commands, window, cx);
            return;
        }
        if let Choice::Skill(package, skill) = choice {
            let reference = Reference {
                label: crate::plugins::metadata::skill_title(&skill),
                target: Target::Skill {
                    package: package.name,
                    name: skill.name,
                },
            };
            self.choose_reference(
                self.references.generation,
                Item::Reference(reference),
                window,
                cx,
            );
            return;
        }
        self.references.dismiss();
        if let Choice::External(entry) = &choice {
            self.references.command_keys.insert(
                entry.declaration.command.as_ref().unwrap().name.clone(),
                entry.key.clone(),
            );
        }
        let replacement = match &choice {
            Choice::Prompt(name) => tr(&format!("composer_prompt_{name}")),
            Choice::Mode(mode) => format!("{} ", mode_token(*mode)).into(),
            Choice::External(entry)
                if entry.declaration.command.as_ref().unwrap().kind
                    == sailry_protocol::plugin::ui::CommandKind::Message =>
            {
                format!("/{} ", entry.declaration.command.as_ref().unwrap().name).into()
            }
            Choice::Skill(..) => format!("{} ", choice.label()).into(),
            _ => "".into(),
        };
        let tagged = matches!(&choice, Choice::Mode(_))
            || matches!(
                &choice,
                Choice::External(entry) if entry.declaration.command.as_ref().unwrap().kind
                    == sailry_protocol::plugin::ui::CommandKind::Message
            );
        self.input.update(cx, |input, cx| {
            if tagged {
                input
                    .replace_range_with_token(trigger.range, token(replacement.trim()), window, cx)
                    .expect("command trigger is a valid input range");
                input.replace(" ", window, cx);
            } else {
                input.set_selected_range(trigger.range, cx);
                input.replace(replacement, window, cx);
            }
            input.focus(window, cx);
        });
        match choice {
            Choice::Model(selection, effort, _) => {
                self.select_model(&selection, effort, window, cx)
            }
            Choice::Reasoning(effort) => self.select_effort(effort, window, cx),
            Choice::Mode(mode) => self.select_mode(mode, window, cx),
            Choice::External(entry) => {
                if entry.declaration.command.as_ref().unwrap().kind
                    == sailry_protocol::plugin::ui::CommandKind::Invoke
                {
                    self.contributions
                        .update(cx, |registry, cx| registry.activate(&entry.key, window, cx));
                }
            }
            Choice::Compact => self.compact_context(window, cx),
            Choice::Settings => cx.emit(Event::Settings(self.settings_node())),
            Choice::Prompt(_) => {}
            Choice::Skill(..) => {}
        }
        cx.notify();
    }
}

pub(in crate::conversation::live) fn token(text: &str) -> gpui_kit::base::input::InlineToken {
    gpui_kit::base::input::InlineToken::new(format!("command:{text}"), text.to_owned())
}

pub(in crate::conversation::live) fn mode_token(mode: sailry_protocol::WorkMode) -> &'static str {
    match mode {
        sailry_protocol::WorkMode::Plan => "/plan",
        sailry_protocol::WorkMode::Code => "/code",
    }
}

/// Only a complete leading token is an instruction, never a path or code sample.
pub(in crate::conversation::live) fn leading(
    text: &str,
) -> Option<(&str, &str, std::ops::Range<usize>)> {
    let source = text.trim_start();
    let start = text.len() - source.len();
    let end = source.find(char::is_whitespace).unwrap_or(source.len());
    let name = source[..end].strip_prefix('/')?;
    sailry_protocol::plugin::ui::command_name(name).then_some((
        name,
        source[end..].trim_start(),
        start..start + end,
    ))
}

pub(in crate::conversation::live) fn leading_mode(
    text: &str,
) -> Option<(sailry_protocol::WorkMode, std::ops::Range<usize>)> {
    let (name, _, range) = leading(text)?;
    let mode = match name {
        "plan" => sailry_protocol::WorkMode::Plan,
        "code" => sailry_protocol::WorkMode::Code,
        _ => return None,
    };
    Some((mode, range))
}
