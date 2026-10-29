//! Display fallbacks for existing picker groups, never channel configuration.
pub use sailry_protocol::conversation::presets::Region;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vendor {
    Qwen,
    Moonshot,
    Xai,
    DeepSeek,
    Groq,
    OpenRouter,
    Fireworks,
    Together,
    Mistral,
    Perplexity,
    Cerebras,
    SambaNova,
    MiniMax,
    Doubao,
    Zhipu,
    Baidu,
    Cohere,
}

impl Vendor {
    pub const ALL: [Self; 17] = [
        Self::Qwen,
        Self::Moonshot,
        Self::Xai,
        Self::DeepSeek,
        Self::Groq,
        Self::OpenRouter,
        Self::Fireworks,
        Self::Together,
        Self::Mistral,
        Self::Perplexity,
        Self::Cerebras,
        Self::SambaNova,
        Self::MiniMax,
        Self::Doubao,
        Self::Zhipu,
        Self::Baidu,
        Self::Cohere,
    ];

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
            _ => "",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Qwen => "provider_qwen",
            Self::Moonshot => "provider_moonshot",
            Self::Xai => "provider_xai",
            Self::DeepSeek => "provider_deepseek",
            Self::Groq => "provider_groq",
            Self::OpenRouter => "provider_openrouter",
            Self::Fireworks => "provider_fireworks",
            Self::Together => "provider_together",
            Self::Mistral => "provider_mistral",
            Self::Perplexity => "provider_perplexity",
            Self::Cerebras => "provider_cerebras",
            Self::SambaNova => "provider_sambanova",
            Self::MiniMax => "provider_minimax",
            Self::Doubao => "provider_doubao",
            Self::Zhipu => "provider_zhipu",
            Self::Baidu => "provider_baidu",
            Self::Cohere => "provider_cohere",
        }
    }
}
