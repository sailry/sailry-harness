use super::*;
use core::prelude::v1::test;

#[test]
fn brand_channels() {
    for (preset, category) in [
        (Preset::CompatibleResponses, ModelCategory::OpenAi),
        (Preset::ChatGptOAuth, ModelCategory::OpenAi),
        (Preset::ChatGpt, ModelCategory::OpenAi),
        (Preset::ClaudeOAuth, ModelCategory::Anthropic),
        (Preset::CompatibleChat, ModelCategory::OpenAi),
        (Preset::CompatibleAnthropic, ModelCategory::Anthropic),
        (Preset::CompatibleGemini, ModelCategory::Gemini),
        (Preset::OpenCodeGo, ModelCategory::OpenCodeGo),
        (Preset::OpenCodeZen, ModelCategory::OpenCodeZen),
    ] {
        assert_eq!(preset.category(), category);
    }
}
