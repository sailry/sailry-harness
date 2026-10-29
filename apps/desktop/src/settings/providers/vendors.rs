//! Display fallbacks for existing picker groups, never channel configuration.
pub use sailry_protocol::conversation::presets::Region;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vendor {
    Qwen,
    Moonshot,
    Xai,
    DeepSeek,
    Mistral,
    MiniMax,
    Doubao,
    Zhipu,
    Baidu,
    Cohere,
}

impl Vendor {
    pub const FIRST_PARTY: [Self; 10] = [
        Self::Xai,
        Self::DeepSeek,
        Self::Qwen,
        Self::Moonshot,
        Self::Mistral,
        Self::MiniMax,
        Self::Doubao,
        Self::Zhipu,
        Self::Baidu,
        Self::Cohere,
    ];

    /// Stable preset identity for the retained picker presentation.
    pub fn id(self) -> &'static str {
        self.key()
            .strip_prefix("provider_")
            .expect("vendor display keys include the preset ID")
    }

    pub fn logo(self) -> &'static str {
        match self {
            Self::Xai => "xai",
            Self::DeepSeek => "deepseek",
            Self::Qwen => "qwen",
            Self::Moonshot => "kimi",
            Self::Mistral => "mistral",
            Self::MiniMax => "minimax",
            Self::Doubao => "doubao",
            Self::Zhipu => "zai",
            Self::Baidu => "wenxin",
            Self::Cohere => "cohere",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Qwen => "provider_qwen",
            Self::Moonshot => "provider_moonshot",
            Self::Xai => "provider_xai",
            Self::DeepSeek => "provider_deepseek",
            Self::Mistral => "provider_mistral",
            Self::MiniMax => "provider_minimax",
            Self::Doubao => "provider_doubao",
            Self::Zhipu => "provider_zhipu",
            Self::Baidu => "provider_baidu",
            Self::Cohere => "provider_cohere",
        }
    }
}
