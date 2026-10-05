// UI contract reference: Sailry Code 67ae9fa0, adapter_catalog.rs and
// ai_channel_editor_dialog.dart. No runtime adapters or credentials are loaded.
use super::vendors::Vendor;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    OpenAi,
    Anthropic,
    Gemini,
    OpenCodeGo,
    OpenCodeZen,
    Azure,
    Bedrock,
    Vertex,
    Hosted(Vendor),
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

    pub fn all() -> impl Iterator<Item = Self> {
        [
            Self::OpenAi,
            Self::Anthropic,
            Self::Gemini,
            Self::OpenCodeGo,
            Self::OpenCodeZen,
            Self::Azure,
            Self::Bedrock,
            Self::Vertex,
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
            Self::Bedrock => "provider_bedrock",
            Self::Vertex => "provider_vertex",
            Self::Hosted(vendor) => vendor.key(),
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
        let name = match self {
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
            Self::Vendor(vendor) => vendor.logo(),
            Self::Other => return Icon::new(IconName::Settings2),
            Self::OpenCodeGo | Self::OpenCodeZen => "opencode",
        };
        Icon::empty().path(format!("icons/providers/{name}.svg"))
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
    OpenAi,
    CompatibleResponses,
    CompatibleChat,
    ChatGpt,
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
    Bedrock,
    BedrockIam,
    Vertex,
    VertexAdc,
    Hosted(Vendor),
}

impl Preset {
    pub fn category(self) -> Category {
        self.family().category()
    }

    pub fn api(self) -> sailry_protocol::conversation::ModelApi {
        use sailry_protocol::conversation::ModelApi;
        match self {
            Self::OpenAi | Self::CompatibleResponses | Self::ChatGpt | Self::CopilotResponses => {
                ModelApi::Responses
            }
            Self::Hosted(Vendor::DeepSeek) => ModelApi::DeepSeek,
            Self::OpenCodeGo => ModelApi::OpenCodeGo,
            Self::OpenCodeZen => ModelApi::OpenCodeZen,
            Self::AzureOpenAi => ModelApi::AzureOpenAi,
            Self::AzureAi => ModelApi::AzureAi,
            Self::Bedrock | Self::BedrockIam => ModelApi::Bedrock,
            Self::Vertex | Self::VertexAdc => ModelApi::Vertex,
            Self::CompatibleChat | Self::Copilot | Self::Hosted(_) => ModelApi::ChatCompletions,
            Self::Anthropic | Self::CompatibleAnthropic => ModelApi::Anthropic,
            Self::Gemini | Self::CompatibleGemini => ModelApi::Gemini,
        }
    }

    const CORE: [Self; 18] = [
        Self::OpenAi,
        Self::CompatibleResponses,
        Self::CompatibleChat,
        Self::ChatGpt,
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
        Self::Bedrock,
        Self::BedrockIam,
        Self::Vertex,
        Self::VertexAdc,
    ];

    pub fn all() -> impl Iterator<Item = Self> {
        Self::CORE.into_iter().chain(Vendor::ALL.map(Self::Hosted))
    }

    pub fn offered(self) -> bool {
        match self {
            Self::Hosted(vendor) => Vendor::FIRST_PARTY.contains(&vendor),
            _ => !self.cloud() && !matches!(self, Self::Copilot | Self::CopilotResponses),
        }
    }

    pub fn family(self) -> Family {
        match self {
            Self::Anthropic | Self::CompatibleAnthropic => Family::Anthropic,
            Self::Gemini | Self::CompatibleGemini => Family::Gemini,
            Self::OpenCodeGo => Family::OpenCodeGo,
            Self::OpenCodeZen => Family::OpenCodeZen,
            Self::AzureOpenAi | Self::AzureAi => Family::Azure,
            Self::Bedrock | Self::BedrockIam => Family::Bedrock,
            Self::Vertex | Self::VertexAdc => Family::Vertex,
            Self::Hosted(vendor) => Family::Hosted(vendor),
            _ => Family::OpenAi,
        }
    }

    pub fn kind(self) -> Self {
        match self {
            Self::CompatibleChat => Self::CompatibleResponses,
            Self::CopilotResponses => Self::Copilot,
            Self::BedrockIam => Self::Bedrock,
            Self::VertexAdc => Self::Vertex,
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
            Self::OpenAi => "preset_openai",
            Self::CompatibleResponses | Self::CompatibleChat => "preset_compatible",
            Self::ChatGpt => "preset_chatgpt",
            Self::Copilot | Self::CopilotResponses => "preset_copilot",
            Self::Anthropic => "preset_anthropic",
            Self::CompatibleAnthropic => "preset_anthropic_compatible",
            Self::Gemini => "preset_gemini",
            Self::OpenCodeGo => "provider_opencode_go",
            Self::OpenCodeZen => "provider_opencode_zen",
            Self::CompatibleGemini => "preset_gemini_compatible",
            Self::AzureOpenAi => "provider_azure_openai",
            Self::AzureAi => "provider_azure_ai",
            Self::Bedrock | Self::BedrockIam => "provider_bedrock",
            Self::Vertex | Self::VertexAdc => "provider_vertex",
            Self::Hosted(vendor) => vendor.key(),
        }
    }

    pub fn oauth(self) -> bool {
        matches!(self, Self::ChatGpt | Self::Copilot | Self::CopilotResponses)
    }

    pub fn cloud(self) -> bool {
        matches!(
            self,
            Self::AzureOpenAi
                | Self::AzureAi
                | Self::Bedrock
                | Self::BedrockIam
                | Self::Vertex
                | Self::VertexAdc
        )
    }

    pub fn key_auth(self) -> bool {
        self.authentication() == sailry_protocol::Authentication::ApiKey
    }

    pub fn custom_endpoint(self) -> bool {
        matches!(
            self,
            Self::CompatibleResponses
                | Self::CompatibleChat
                | Self::CompatibleAnthropic
                | Self::CompatibleGemini
                | Self::AzureOpenAi
                | Self::AzureAi
        )
    }

    pub fn endpoint_hint(self) -> &'static str {
        if matches!(
            self,
            Self::Bedrock | Self::BedrockIam | Self::Vertex | Self::VertexAdc
        ) {
            return "provider_cloud_endpoint_hint";
        }
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
            Self::ChatGpt => Authentication::ChatGpt,
            Self::BedrockIam | Self::VertexAdc => Authentication::Host,
            Self::Copilot | Self::CopilotResponses => Authentication::Copilot,
            _ => Authentication::ApiKey,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    pub id: String,
    pub context: u32,
    pub output: u32,
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
    pub fn example(id: &str) -> Self {
        Self {
            id: id.into(),
            context: 128_000,
            output: 16_384,
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
            default_effort: sailry_protocol::Effort::Medium,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Channel {
    pub options: Option<sailry_protocol::conversation::cloud::Options>,
    pub oauth: Option<sailry_protocol::conversation::oauth::Options>,
    pub id: usize,
    pub name: String,
    pub preset: Preset,
    pub endpoint: String,
    pub credential_configured: bool,
    pub enabled: bool,
    pub connected: bool,
    pub models: Vec<Model>,
    pub default_model: String,
}

pub struct Store {
    pub category: Category,
    pub channels: Vec<Channel>,
    pub catalog_revision: usize,
    next_id: usize,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            category: Category::OpenAi,
            channels: vec![Channel {
                options: None,
                oauth: None,
                id: 0,
                name: crate::tr("settings_sample_provider").to_string(),
                preset: Preset::OpenAi,
                endpoint: String::new(),
                credential_configured: true,
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
