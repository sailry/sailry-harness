// Endpoint reference: ADK-Rust 7cf2dc7, adk-model's OpenAI-compatible
// presets and the DeepSeek, Groq, and OpenRouter configurations.
// DeepSeek uses its native adapter; the remaining entries use the shared
// Chat Completions adapter without cloud identity authentication.
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

    pub fn endpoint(self) -> &'static str {
        match self {
            Self::Qwen => "https://dashscope.aliyuncs.com/compatible-mode/v1",
            Self::Moonshot => "https://api.moonshot.ai/v1",
            Self::Xai => "https://api.x.ai/v1",
            Self::DeepSeek => "https://api.deepseek.com",
            Self::Groq => "https://api.groq.com/openai/v1",
            Self::OpenRouter => "https://openrouter.ai/api/v1",
            Self::Fireworks => "https://api.fireworks.ai/inference/v1",
            Self::Together => "https://api.together.xyz/v1",
            Self::Mistral => "https://api.mistral.ai/v1",
            Self::Perplexity => "https://api.perplexity.ai",
            Self::Cerebras => "https://api.cerebras.ai/v1",
            Self::SambaNova => "https://api.sambanova.ai/v1",
            Self::MiniMax => "https://api.minimax.chat/v1",
            Self::Doubao => "https://ark.cn-beijing.volces.com/api/v3",
            Self::Zhipu => "https://open.bigmodel.cn/api/paas/v4",
            Self::Baidu => "https://qianfan.baidubce.com/v2",
            Self::Cohere => "https://api.cohere.com/compatibility/v1",
        }
    }

    pub fn from_endpoint(endpoint: &str) -> Option<Self> {
        let endpoint = endpoint.trim_end_matches('/');
        Self::ALL
            .into_iter()
            .find(|vendor| vendor.endpoint() == endpoint)
    }
}
