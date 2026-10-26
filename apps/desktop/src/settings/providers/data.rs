// UI contract reference: Sailry Code 67ae9fa0, adapter_catalog.rs and
// ai_channel_editor_dialog.dart. No runtime adapters or credentials are loaded.
use super::vendors::{Region, Vendor};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    OpenAi,
    Anthropic,
    Gemini,
    OpenCodeGo,
    OpenCodeZen,
    Azure,
    Hosted(Vendor),
    Other,
}

impl Family {
    pub fn category(self) -> Category {
        match self {
            Self::OpenAi => Category::OpenAi,
            Self::Anthropic => Category::Anthropic,
            Self::Gemini => Category::Gemini,
            Self::OpenCodeGo => Category::OpenCodeGo,
            Self::OpenCodeZen => Category::OpenCodeZen,
            Self::Hosted(vendor) if Vendor::FIRST_PARTY.contains(&vendor) => {
                Category::Vendor(vendor)
            }
            _ => Category::Other,
        }
    }

    #[cfg(test)]
    pub fn all() -> impl Iterator<Item = Self> {
        [
            Self::OpenAi,
            Self::Anthropic,
            Self::Gemini,
            Self::OpenCodeGo,
            Self::OpenCodeZen,
            Self::Azure,
            Self::Other,
        ]
        .into_iter()
        .chain(Vendor::ALL.map(Self::Hosted))
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::OpenAi => "provider_openai",
            Self::Anthropic => "provider_anthropic",
            Self::Gemini => "provider_gemini",
            Self::OpenCodeGo => "provider_opencode_go",
            Self::OpenCodeZen => "provider_opencode_zen",
            Self::Azure => "provider_azure",
            Self::Hosted(vendor) => vendor.key(),
            Self::Other => "provider_other",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    OpenAi,
    Anthropic,
    Gemini,
    OpenCodeGo,
    OpenCodeZen,
    Vendor(Vendor),
    Other,
}

impl Category {
    pub const ALL: [Self; 16] = [
        Self::OpenAi,
        Self::Anthropic,
        Self::Gemini,
        Self::OpenCodeGo,
        Self::OpenCodeZen,
        Self::Vendor(Vendor::Xai),
        Self::Vendor(Vendor::DeepSeek),
        Self::Vendor(Vendor::Qwen),
        Self::Vendor(Vendor::Moonshot),
        Self::Vendor(Vendor::Mistral),
        Self::Vendor(Vendor::MiniMax),
        Self::Vendor(Vendor::Doubao),
        Self::Vendor(Vendor::Zhipu),
        Self::Vendor(Vendor::Baidu),
        Self::Vendor(Vendor::Cohere),
        Self::Other,
    ];

    pub fn icon(self) -> gpui_kit::component::Icon {
        use gpui_kit::component::{Icon, IconName};
        self.logo().map_or_else(
            || Icon::new(IconName::Settings2),
            |path| Icon::empty().path(path),
        )
    }

    /// Logo asset path; generic providers have none.
    pub fn logo(self) -> Option<String> {
        let name = match self {
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
            Self::Vendor(vendor) => vendor.logo(),
            Self::Other => return None,
            Self::OpenCodeGo | Self::OpenCodeZen => "opencode",
        };
        Some(format!("icons/providers/{name}.svg"))
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::OpenAi => "provider_openai",
            Self::Anthropic => "provider_anthropic",
            Self::Gemini => "provider_gemini",
            Self::OpenCodeGo => "provider_opencode_go",
            Self::OpenCodeZen => "provider_opencode_zen",
            Self::Vendor(vendor) => vendor.key(),
            Self::Other => "provider_other",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Plugin,
    OpenAi,
    CompatibleResponses,
    CompatibleChat,
    ChatGptOAuth,
    ChatGpt,
    ClaudeOAuth,
    Copilot,
    CopilotResponses,
    Anthropic,
    CompatibleAnthropic,
    Gemini,
    OpenCodeGo,
    OpenCodeZen,
    CompatibleGemini,
    AzureOpenAi,
    AzureAi,
    Hosted(Vendor),
}

impl Preset {
    pub fn category(self) -> Category {
        self.family().category()
    }

    pub fn api(self) -> sailry_protocol::conversation::ModelApi {
        use sailry_protocol::conversation::ModelApi;
        match self {
            Self::Plugin => ModelApi::Plugin,
            Self::OpenAi
            | Self::CompatibleResponses
            | Self::ChatGptOAuth
            | Self::ChatGpt
            | Self::CopilotResponses => ModelApi::Responses,
            Self::Hosted(Vendor::DeepSeek) => ModelApi::DeepSeek,
            Self::OpenCodeGo => ModelApi::OpenCodeGo,
            Self::OpenCodeZen => ModelApi::OpenCodeZen,
            Self::AzureOpenAi => ModelApi::AzureOpenAi,
            Self::AzureAi => ModelApi::AzureAi,
            Self::CompatibleChat | Self::Copilot | Self::Hosted(_) => ModelApi::ChatCompletions,
            Self::Anthropic | Self::CompatibleAnthropic | Self::ClaudeOAuth => ModelApi::Anthropic,
            Self::Gemini | Self::CompatibleGemini => ModelApi::Gemini,
        }
    }

    #[cfg(test)]
    const CORE: [Self; 17] = [
        Self::Plugin,
        Self::OpenAi,
        Self::CompatibleResponses,
        Self::CompatibleChat,
        Self::ChatGptOAuth,
        Self::ChatGpt,
        Self::ClaudeOAuth,
        Self::Copilot,
        Self::CopilotResponses,
        Self::Anthropic,
        Self::CompatibleAnthropic,
        Self::Gemini,
        Self::OpenCodeGo,
        Self::OpenCodeZen,
        Self::CompatibleGemini,
        Self::AzureOpenAi,
        Self::AzureAi,
    ];

    #[cfg(test)]
    pub fn all() -> impl Iterator<Item = Self> {
        Self::CORE.into_iter().chain(Vendor::ALL.map(Self::Hosted))
    }

    pub fn family(self) -> Family {
        match self {
            Self::Anthropic | Self::CompatibleAnthropic | Self::ClaudeOAuth => Family::Anthropic,
            Self::Gemini | Self::CompatibleGemini => Family::Gemini,
            Self::OpenCodeGo => Family::OpenCodeGo,
            Self::OpenCodeZen => Family::OpenCodeZen,
            Self::AzureOpenAi | Self::AzureAi => Family::Azure,
            Self::Hosted(vendor) => Family::Hosted(vendor),
            Self::Plugin => Family::Other,
            _ => Family::OpenAi,
        }
    }

    pub fn kind(self) -> Self {
        match self {
            Self::CompatibleChat => Self::CompatibleResponses,
            Self::CopilotResponses => Self::Copilot,
            _ => self,
        }
    }

    pub fn protocols(self) -> &'static [Self] {
        match self.kind() {
            Self::CompatibleResponses => &[Self::CompatibleResponses, Self::CompatibleChat],
            Self::Copilot => &[Self::Copilot, Self::CopilotResponses],
            _ => &[],
        }
    }

    pub fn protocol_key(self) -> &'static str {
        match self {
            Self::CompatibleChat | Self::Copilot => "provider_api_chat",
            _ => "provider_api_responses",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Plugin => "provider_plugin",
            Self::OpenAi => "preset_openai",
            Self::CompatibleResponses | Self::CompatibleChat => "preset_compatible",
            Self::ChatGptOAuth => "preset_chatgpt_oauth",
            Self::ChatGpt => "preset_chatgpt",
            Self::ClaudeOAuth => "preset_claude_oauth",
            Self::Copilot | Self::CopilotResponses => "preset_copilot",
            Self::Anthropic => "preset_anthropic",
            Self::CompatibleAnthropic => "preset_anthropic_compatible",
            Self::Gemini => "preset_gemini",
            Self::OpenCodeGo => "provider_opencode_go",
            Self::OpenCodeZen => "provider_opencode_zen",
            Self::CompatibleGemini => "preset_gemini_compatible",
            Self::AzureOpenAi => "provider_azure_openai",
            Self::AzureAi => "provider_azure_ai",
            Self::Hosted(vendor) => vendor.key(),
        }
    }

    pub fn oauth(self) -> bool {
        matches!(
            self,
            Self::ChatGptOAuth
                | Self::ChatGpt
                | Self::ClaudeOAuth
                | Self::Copilot
                | Self::CopilotResponses
        )
    }

    pub fn cloud(self) -> bool {
        matches!(self, Self::AzureOpenAi | Self::AzureAi)
    }

    pub fn key_auth(self) -> bool {
        self.authentication() == sailry_protocol::Authentication::ApiKey
    }

    pub fn custom_endpoint(self) -> bool {
        matches!(
            self,
            Self::Plugin
                | Self::CompatibleResponses
                | Self::CompatibleChat
                | Self::CompatibleAnthropic
                | Self::CompatibleGemini
                | Self::AzureOpenAi
                | Self::AzureAi
        )
    }

    pub fn endpoint_hint(self) -> &'static str {
        match self.api() {
            sailry_protocol::conversation::ModelApi::AzureOpenAi => "provider_azure_openai_hint",
            sailry_protocol::conversation::ModelApi::AzureAi => "provider_azure_ai_hint",
            sailry_protocol::conversation::ModelApi::Anthropic => {
                "provider_endpoint_anthropic_hint"
            }
            sailry_protocol::conversation::ModelApi::Gemini => "provider_endpoint_gemini_hint",
            _ => "provider_endpoint_hint",
        }
    }

    pub fn web_search(self) -> bool {
        matches!(
            self,
            Self::OpenAi
                | Self::CompatibleResponses
                | Self::Anthropic
                | Self::CompatibleAnthropic
                | Self::Gemini
                | Self::CompatibleGemini
        )
    }

    pub fn authentication(self) -> sailry_protocol::Authentication {
        use sailry_protocol::Authentication;
        match self {
            Self::ChatGptOAuth => Authentication::ChatGptOAuth,
            Self::ChatGpt => Authentication::ChatGpt,
            Self::ClaudeOAuth => Authentication::ClaudeOAuth,
            Self::Copilot | Self::CopilotResponses => Authentication::Copilot,
            _ => Authentication::ApiKey,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    pub id: String,
    pub display_name: Option<String>,
    pub context: u32,
    pub output: u32,
    pub compaction_tokens: Option<u32>,
    pub capabilities: Option<sailry_protocol::conversation::discovery::Capabilities>,
    pub vision: bool,
    pub tools: bool,
    pub reasoning: bool,
    pub web_search: bool,
    pub generates: Vec<sailry_protocol::media::Generation>,
    pub efforts: Vec<sailry_protocol::Effort>,
    pub custom_efforts: bool,
    pub default_effort: sailry_protocol::Effort,
}

impl Model {
    pub fn label(&self) -> &str {
        self.display_name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(&self.id)
    }

    pub fn example(id: &str) -> Self {
        Self {
            id: id.into(),
            display_name: None,
            context: 128_000,
            output: 16_384,
            capabilities: None,
            vision: true,
            tools: true,
            reasoning: true,
            web_search: false,
            generates: vec![],
            efforts: vec![
                sailry_protocol::Effort::Low,
                sailry_protocol::Effort::Medium,
                sailry_protocol::Effort::High,
            ],
            custom_efforts: false,
            compaction_tokens: None,
            default_effort: sailry_protocol::Effort::Medium,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Channel {
    pub directory: Option<sailry_protocol::conversation::presets::Entry>,
    pub options: Option<sailry_protocol::conversation::connection::Options>,
    pub oauth: Option<sailry_protocol::conversation::oauth::Options>,
    pub quota: Option<sailry_protocol::conversation::quota::Snapshot>,
    pub id: usize,
    pub name: String,
    pub preset: Preset,
    pub endpoint: String,
    pub credential_configured: bool,
    pub account_label: Option<String>,
    pub enabled: bool,
    pub connected: bool,
    pub models: Vec<Model>,
    pub default_model: String,
}

impl Channel {
    pub fn label(&self) -> gpui_kit::SharedString {
        use sailry_protocol::conversation::connection::{Options, Product};
        if let Some(entry) = self
            .directory
            .as_ref()
            .filter(|entry| entry.source != sailry_protocol::conversation::presets::Source::Builtin)
        {
            return super::presets::label(&entry.preset).into();
        }
        let Preset::Hosted(vendor) = self.preset else {
            return crate::tr(self.preset.key());
        };
        let key = if matches!(
            self.options,
            Some(Options::Hosted {
                product: Product::Coding
            })
        ) {
            vendor.coding_key()
        } else {
            self.preset.key()
        };
        let label = crate::tr(key);
        vendor
            .region(&self.endpoint)
            .map_or(label.clone(), |region| {
                let key = match region {
                    Region::China => "provider_region_china",
                    Region::Global => "provider_region_global",
                };
                format!("{label} {}", crate::tr(key)).into()
            })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    All,
    Group(String),
}

pub struct Store {
    pub category: Selection,
    /// The channel whose models are listed inline.
    pub expanded: Option<usize>,
    pub channels: Vec<Channel>,
    pub catalog_revision: usize,
    next_id: usize,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            category: Selection::All,
            expanded: None,
            channels: vec![Channel {
                directory: None,
                options: None,
                oauth: None,
                quota: None,
                id: 0,
                name: crate::tr("settings_sample_provider").to_string(),
                preset: Preset::OpenAi,
                endpoint: String::new(),
                credential_configured: true,
                account_label: None,
                enabled: true,
                connected: false,
                models: vec![Model::example("preview-text-1")],
                default_model: "preview-text-1".into(),
            }],
            catalog_revision: 0,
            next_id: 1,
        }
    }
}

impl Store {
    pub fn save(&mut self, editing: Option<usize>, mut channel: Channel) {
        if let Some(existing) = editing.and_then(|id| self.channels.iter_mut().find(|c| c.id == id))
        {
            channel.id = existing.id;
            channel.enabled = existing.enabled;
            channel.connected = existing.connected;
            *existing = channel;
        } else if editing.is_none() {
            channel.id = self.next_id;
            self.next_id += 1;
            self.channels.push(channel);
        }
        self.channels
            .sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    }

    pub fn remove(&mut self, id: usize) {
        self.channels.retain(|channel| channel.id != id);
    }
}
