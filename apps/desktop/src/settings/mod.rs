mod appearance;
mod backup;
mod catalog;
mod connections;
pub(crate) use connections::Connections as Pairing;
pub(crate) use connections::edit::open as edit_host_network;
mod conversation;
mod dictation;
mod draft;
mod entry;
pub(crate) mod group;
mod hooks;
mod knowledge;
mod market;
mod navigation;
pub(crate) use navigation::OpenSection;
mod mcp;
mod model_library;
mod plugins;
pub(crate) use navigation::{CloseDetails, OpenDetails};
pub(crate) use plugins::open_bound as open_plugin_details;
pub(crate) use plugins::preparation::label as python_stage;
pub(crate) use plugins::updates::connected as check_plugin_updates;
mod preferences;
mod providers;
mod resource_card;
pub(crate) use providers::{Channel, Model, ModelCategory};
pub(crate) use providers::{channel, logo as provider_logo};
mod roles;
mod runtime;
pub(crate) use roles::Role;
pub(crate) mod extensions;
mod shortcuts;
mod skills;
mod terminal;
pub(super) mod upload;

use gpui_kit::component::{scroll::ScrollableElement, *};
use gpui_kit::*;

use crate::tr;

pub(crate) fn open_plugin_configuration(
    binding: crate::conversation::live::Binding,
    expected: sailry_protocol::plugin::Summary,
    window: &mut Window,
    cx: &mut App,
) {
    plugins::configuration::mount(
        providers::Binding {
            client: binding.client,
            runtime: binding.runtime,
            label: binding.host,
        },
        expected,
        window,
        cx,
    );
}

pub(crate) fn open_plugin_settings(
    binding: crate::conversation::live::Binding,
    info: sailry_protocol::plugin::Info,
    window: &mut Window,
    cx: &mut App,
) {
    extensions::modal::open(
        providers::Binding {
            client: binding.client,
            runtime: binding.runtime,
            label: binding.host,
        },
        info,
        window,
        cx,
    );
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Section {
    #[default]
    General,
    Appearance,
    Conversation,
    Terminal,
    Knowledge,
    Runtime,
    Ocr,
    Backup,
    Hooks,
    Plugin,
    Market,
    Plugins,
    Mcp,
    Providers,
    Skills,
    Connections,
    Shortcuts,
    Dictation,
    Permissions,
    About,
}

impl Section {
    pub(crate) const EXTENSIONS: [Self; 4] = [Self::Market, Self::Plugins, Self::Skills, Self::Mcp];

    pub(crate) fn is_extension(self) -> bool {
        Self::EXTENSIONS.contains(&self)
    }

    pub const GROUPS: [(&'static str, &'static [Self]); 4] = [
        (
            "settings_group_app",
            &[
                Self::General,
                Self::Appearance,
                Self::Conversation,
                Self::Shortcuts,
                Self::Dictation,
            ],
        ),
        ("settings_group_ai", &[Self::Providers, Self::Knowledge]),
        (
            "settings_group_tools",
            &[
                Self::Terminal,
                Self::Connections,
                Self::Hooks,
                Self::Runtime,
                Self::Ocr,
            ],
        ),
        (
            "settings_group_system",
            &[Self::Permissions, Self::Backup, Self::About],
        ),
    ];

    pub(crate) fn navigation_order(self) -> u16 {
        match self {
            Self::General | Self::Providers | Self::Terminal | Self::Permissions => 100,
            Self::About | Self::Knowledge => 200,
            Self::Backup => 300,
            Self::Appearance | Self::Connections => 200,
            Self::Conversation => 300,
            Self::Shortcuts => 400,
            Self::Hooks => 500,
            Self::Dictation => 500,
            Self::Runtime => 600,
            Self::Ocr => 700,
            _ => 10_000,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::General => "settings_general",
            Self::Appearance => "settings_appearance",
            Self::Conversation => "settings_conversation",
            Self::Terminal => "settings_terminal",
            Self::Knowledge => "settings_knowledge",
            Self::Runtime => "settings_runtime",
            Self::Ocr => "settings_ocr",
            Self::Backup => "settings_backup",
            Self::Hooks => "hooks_settings",
            Self::Plugin => "plugins_settings_title",
            Self::Market => "plugins_market",
            Self::Plugins => "settings_plugins",
            Self::Mcp => "settings_mcp",
            Self::Providers => "settings_providers",
            Self::Skills => "settings_skills",
            Self::Connections => "settings_connections",
            Self::Shortcuts => "settings_shortcuts",
            Self::Dictation => "settings_dictation",
            Self::Permissions => "settings_permissions",
            Self::About => "settings_about",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Self::General => IconName::Settings,
            Self::Appearance => IconName::Palette,
            Self::Conversation => return Icon::default().path("reicon:messages/chat-round"),
            Self::Terminal => IconName::SquareTerminal,
            Self::Knowledge => IconName::BookOpen,
            Self::Runtime => IconName::Cpu,
            Self::Ocr => IconName::ALargeSmall,
            Self::Backup => return gpui_kit::assets::IconName::Upload.into(),
            Self::Hooks => IconName::Settings2,
            Self::Plugin => IconName::Settings2,
            Self::Market | Self::Plugins => IconName::GalleryVerticalEnd,
            Self::Mcp | Self::Providers => IconName::Cpu,
            Self::Skills => IconName::Bot,
            Self::Connections => IconName::Network,
            Self::Shortcuts => IconName::ALargeSmall,
            Self::Dictation => IconName::ALargeSmall,
            Self::Permissions => IconName::Settings2,
            Self::About => IconName::Info,
        }
        .into()
    }
}

pub(crate) struct OpenPlugins;
impl EventEmitter<OpenPlugins> for Workspace {}

pub struct Workspace {
    connections: Entity<connections::Connections>,
    pub section: Section,
    providers: providers::Store,
    provider_link: Option<providers::Live>,
    terminal: terminal::State,
    knowledge: knowledge::State,
    runtime: runtime::State,
    backup: backup::State,
    hooks: hooks::State,
    conversation: conversation::State,
    opacity: appearance::opacity::State,
    theme_focus: FocusHandle,
    shortcuts: Entity<shortcuts::Panel>,
    dictation: Entity<dictation::Panel>,
    permissions: Entity<crate::permissions::Panel>,
    updater: Entity<crate::updater::Panel>,
    pub(crate) usage_sources: Vec<crate::plugins::usage::Source>,
    toast_duration: Entity<gpui_kit::component::select::SelectState<Vec<preferences::Duration>>>,
    language: Entity<gpui_kit::component::select::SelectState<Vec<preferences::Language>>>,
    extensions: extensions::State,
    roles: Vec<roles::Role>,
    role_catalog: roles::Catalog,
    skills: Vec<skills::Skill>,
    skill_state: skills::State,
    plugins: Vec<plugins::Plugin>,
    plugin_serial: usize,
    plugin_catalog: plugins::Catalog,
}

impl Workspace {
    pub(crate) fn model_channels(&self) -> &[Channel] {
        &self.providers.channels
    }

    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut workspace = Self::unbound(window, cx);
        providers::bind_local(&mut workspace, cx);
        workspace
    }

    fn unbound(window: &mut Window, cx: &mut Context<Self>) -> Self {
        crate::preferences::init(cx);
        crate::feedback::observe_with(
            window,
            cx,
            |view: &Self, cx| {
                [
                    view.plugin_catalog.error,
                    view.skill_state.error,
                    view.provider_link.as_ref().and_then(|live| live.error),
                    view.provider_link
                        .as_ref()
                        .and_then(|live| live.catalog.error),
                    cx.try_global::<crate::preferences::Preferences>()
                        .and_then(|preferences| preferences.error),
                    cx.try_global::<crate::theme::Catalog>()
                        .and_then(|catalog| catalog.error),
                ]
                .into_iter()
                .flatten()
                .collect()
            },
            |_, key, _| {
                let notice = gpui_kit::component::notification::Notification::error(crate::tr(key));
                if key == "preferences_save_failed" {
                    notice.action(|_, _, cx| {
                        crate::feedback::retry("preferences-retry").on_click(cx.listener(
                            |toast, _, window, cx| {
                                toast.dismiss(window, cx);
                                crate::preferences::update(cx, |_| {});
                            },
                        ))
                    })
                } else {
                    notice
                }
            },
        );
        cx.observe_global::<crate::preferences::Preferences>(|_, cx| cx.notify())
            .detach();
        cx.observe_global::<crate::theme::Catalog>(|_, cx| cx.notify())
            .detach();
        cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() && this.section == Section::Dictation {
                this.dictation.update(cx, |panel, cx| panel.refresh(cx));
            }
        })
        .detach();
        let terminal = terminal::State::new(window, cx);
        cx.observe_window_appearance(window, |_, window, cx| {
            if crate::theme::follows_system(cx) {
                Theme::sync_system_appearance(Some(window), cx);
            }
        })
        .detach();
        cx.observe(&terminal.font_size, |_, _, cx| cx.notify())
            .detach();
        Self {
            connections: cx.new(|cx| connections::Connections::new(window, cx)),
            section: Section::General,
            providers: providers::Store::default(),
            provider_link: None,
            terminal,
            knowledge: knowledge::State::default(),
            runtime: runtime::State::default(),
            backup: backup::State::default(),
            hooks: hooks::State::default(),
            conversation: conversation::State::new(window, cx),
            opacity: appearance::opacity::State::new(cx),
            theme_focus: cx.focus_handle(),
            shortcuts: cx.new(shortcuts::Panel::new),
            dictation: cx.new(dictation::Panel::new),
            permissions: cx.new(|cx| crate::permissions::Panel::new(window, cx)),
            updater: cx.new(|cx| crate::updater::Panel::new(window, cx)),
            usage_sources: vec![],
            toast_duration: preferences::duration(window, cx),
            language: preferences::language(window, cx),
            extensions: Default::default(),
            roles: vec![roles::Role::example()],
            role_catalog: roles::Catalog::default(),
            skills: skills::Skill::examples(),
            skill_state: skills::State::new(window, cx),
            plugins: vec![plugins::Plugin::example()],
            plugin_serial: 0,
            plugin_catalog: plugins::Catalog::default(),
        }
    }

    pub fn select(&mut self, section: Section, cx: &mut Context<Self>) {
        if section != Section::Connections {
            self.deactivate(cx);
        } else {
            self.shortcuts.update(cx, |panel, cx| panel.cancel(cx));
        }
        self.section = section;
        self.permissions.update(cx, |panel, cx| {
            panel.activate(section == Section::Permissions, cx)
        });
        if let Some(metadata) = &self.plugin_catalog.metadata {
            metadata.update(cx, |metadata, cx| metadata.refresh(cx));
        }
        if section == Section::Dictation {
            self.dictation.update(cx, |panel, cx| panel.refresh(cx));
        }
        if section == Section::Providers {
            self.refresh_catalog(cx);
        }
        if section.is_extension() {
            cx.emit(OpenPlugins);
        }
        cx.notify();
    }

    pub fn leave(&mut self, cx: &mut Context<Self>) {
        self.deactivate(cx);
        self.extensions.clear_resources();
    }

    pub fn deactivate(&mut self, cx: &mut Context<Self>) {
        self.clear_knowledge(cx);
        self.clear_runtime(cx);
        self.clear_backup(cx);
        self.clear_hooks(cx);
        self.permissions
            .update(cx, |panel, cx| panel.activate(false, cx));
        self.extensions.park(cx);
        self.shortcuts.update(cx, |panel, cx| panel.cancel(cx));
        self.connections
            .update(cx, |connections, cx| connections.close(cx));
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.warm_settings(cx);
        if self.section == Section::Plugins && self.provider_link.is_some() {
            return self.inventory(window, cx);
        }
        let content = match self.section {
            Section::Connections => self.connections.clone().into_any_element(),
            Section::Providers => self.providers(cx),
            Section::Plugin => self.plugin_settings(window, cx),
            Section::General => self.preferences(window, cx),
            Section::Appearance => self.appearance(cx),
            Section::Conversation => self.conversation(window, cx),
            Section::Terminal => self.terminal(window, cx),
            Section::Knowledge => self.knowledge(window, cx),
            Section::Runtime | Section::Ocr => self.runtime(window, cx),
            Section::Backup => self.backup(window, cx),
            Section::Hooks => self.hooks.render(window, cx),
            Section::Shortcuts => self.shortcuts.clone().into_any_element(),
            Section::Dictation => {
                let binding = self.provider_link.as_ref().map(|live| live.binding.clone());
                self.dictation
                    .update(cx, |panel, cx| panel.bind(binding, window, cx));
                self.dictation.clone().into_any_element()
            }
            Section::Permissions => self.permissions.clone().into_any_element(),
            Section::Market => self.plugin_page(window, cx),
            Section::Plugins => self.plugins(cx),
            Section::Mcp => self.mcp(cx),
            Section::Skills => self.skills(cx),
            _ => self.catalog(cx),
        };
        if self.section == Section::Market
            && let Some(market) = &self.plugin_catalog.market
        {
            let scroll = market.read(cx).viewport();
            return v_flex()
                .id("market-viewport")
                .size_full()
                .min_h_0()
                .min_w_0()
                .overflow_y_scroll()
                .lock_scroll_axis()
                .track_scroll(&scroll)
                .child(div().flex_none().w_full().min_h_full().p_6().child(content))
                .vertical_scrollbar(&scroll)
                .into_any_element();
        }
        if self.plugin_full_width(cx) {
            return div()
                .size_full()
                .min_h_0()
                .min_w_0()
                .debug_selector(|| "settings-content".into())
                .child(content)
                .into_any_element();
        }
        crate::ui::settings_page::SettingsPage::new(
            "settings",
            if self.section == Section::Plugin {
                self.plugin_heading(cx).into()
            } else {
                tr(self.section.key())
            },
        )
        .key(self.section as usize)
        .leading(crate::theme::banner("banner.settings", cx))
        .description(if self.section == Section::Plugin {
            self.plugin_description(cx).into()
        } else {
            tr(&format!("{}_description", self.section.key()))
        })
        .child(content)
        .into_any_element()
    }
}
