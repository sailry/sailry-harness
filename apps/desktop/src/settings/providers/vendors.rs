//! Presentation identities for existing channels; route data has one catalog owner.
pub use sailry_protocol::conversation::presets::Region;
use sailry_protocol::conversation::{connection::Product, presets::Preset};
use std::sync::OnceLock;

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

    fn declaration(self) -> &'static Preset {
        super::presets::catalog()
            .builtin
            .iter()
            .find(|preset| Some(preset.id.as_str()) == self.key().strip_prefix("provider_"))
            .expect("existing provider identity must have a bundled declaration")
    }

    fn routes(self) -> &'static Routes {
        static ROUTES: OnceLock<Vec<(Vendor, Routes)>> = OnceLock::new();
        ROUTES
            .get_or_init(|| {
                Self::ALL
                    .into_iter()
                    .map(|vendor| {
                        let preset = vendor.declaration();
                        let regional = |product| {
                            preset
                                .routes
                                .iter()
                                .filter(|route| route.product == Some(product))
                                .filter_map(|route| {
                                    route.region.map(|region| (region, route.endpoint.as_str()))
                                })
                                .collect()
                        };
                        (
                            vendor,
                            Routes {
                                endpoint: preset
                                    .routes
                                    .first()
                                    .expect("bundled routes are nonempty")
                                    .endpoint
                                    .as_str(),
                                metered: regional(Product::Metered),
                                coding: regional(Product::Coding),
                                model: preset
                                    .products
                                    .iter()
                                    .find(|product| product.kind == Product::Coding)
                                    .and_then(|product| product.default_model.as_deref()),
                            },
                        )
                    })
                    .collect()
            })
            .iter()
            .find(|(vendor, _)| *vendor == self)
            .map(|(_, routes)| routes)
            .expect("existing provider routes must be declared")
    }

    pub fn endpoint(self) -> &'static str {
        self.routes().endpoint
    }

    pub fn endpoints(self) -> &'static [(Region, &'static str)] {
        &self.routes().metered
    }

    pub fn coding_endpoints(self) -> &'static [(Region, &'static str)] {
        &self.routes().coding
    }

    pub fn region(self, endpoint: &str) -> Option<Region> {
        let endpoint = endpoint.trim_end_matches('/');
        if self == Self::MiniMax && endpoint == "https://api.minimaxi.com/v1" {
            return Some(Region::China);
        }
        self.endpoints()
            .iter()
            .chain(self.coding_endpoints())
            .find_map(|(region, url)| (*url == endpoint).then_some(*region))
    }

    pub fn coding_key(self) -> &'static str {
        match self {
            Self::Qwen => "provider_qwen_coding",
            Self::Moonshot => "provider_kimi_code",
            Self::MiniMax => "provider_minimax_coding",
            Self::Zhipu => "provider_glm_coding",
            Self::Doubao => "provider_ark_coding",
            _ => "provider_coding",
        }
    }

    pub fn coding_model(self) -> Option<&'static str> {
        self.routes().model
    }

    pub fn from_endpoint(endpoint: &str) -> Option<Self> {
        let endpoint = endpoint.trim_end_matches('/');
        if matches!(
            endpoint,
            "https://api.minimax.chat/v1" | "https://api.minimaxi.com/v1"
        ) {
            return Some(Self::MiniMax);
        }
        Self::ALL.into_iter().find(|vendor| {
            vendor.endpoint() == endpoint
                || vendor
                    .endpoints()
                    .iter()
                    .chain(vendor.coding_endpoints())
                    .any(|(_, url)| *url == endpoint)
        })
    }
}

struct Routes {
    endpoint: &'static str,
    metered: Vec<(Region, &'static str)>,
    coding: Vec<(Region, &'static str)>,
    model: Option<&'static str>,
}

#[cfg(test)]
mod tests;
