use serde::{Deserialize, Serialize, de};

/// Setting for safety
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetySetting {
    /// The category of content to filter
    pub category: HarmCategory,
    /// The threshold for filtering
    pub threshold: HarmBlockThreshold,
}

/// Category of harmful content
#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum HarmCategory {
    /// Category is unspecified.
    #[serde(rename = "HARM_CATEGORY_UNSPECIFIED")]
    Unspecified,
    /// PaLM - Negative or harmful comments targeting identity and/or protected attribute.
    #[serde(rename = "HARM_CATEGORY_DEROGATORY")]
    Derogatory,
    /// PaLM - Content that is rude, disrespectful, or profane.
    #[serde(rename = "HARM_CATEGORY_TOXICITY")]
    Toxicity,
    /// PaLM - Describes scenarios depicting violence against an individual or group, or general descriptions of gore.
    #[serde(rename = "HARM_CATEGORY_VIOLENCE")]
    Violence,
    /// PaLM - Contains references to sexual acts or other lewd content.
    #[serde(rename = "HARM_CATEGORY_SEXUAL")]
    Sexual,
    /// PaLM - Promotes unchecked medical advice.
    #[serde(rename = "HARM_CATEGORY_MEDICAL")]
    Medical,
    /// PaLM - Dangerous content that promotes, facilitates, or encourages harmful acts.
    #[serde(rename = "HARM_CATEGORY_DANGEROUS")]
    Dangerous,
    /// Gemini - Harassment content.
    #[serde(rename = "HARM_CATEGORY_HARASSMENT")]
    Harassment,
    /// Gemini - Hate speech and content.
    #[serde(rename = "HARM_CATEGORY_HATE_SPEECH")]
    HateSpeech,
    /// Gemini - Sexually explicit content.
    #[serde(rename = "HARM_CATEGORY_SEXUALLY_EXPLICIT")]
    SexuallyExplicit,
    /// Gemini - Dangerous content.
    #[serde(rename = "HARM_CATEGORY_DANGEROUS_CONTENT")]
    DangerousContent,
    /// Gemini - Civic integrity content.
    #[serde(rename = "HARM_CATEGORY_CIVIC_INTEGRITY")]
    CivicIntegrity,
    /// Gemini - Jailbreak-related content.
    #[serde(rename = "HARM_CATEGORY_JAILBREAK")]
    Jailbreak,
}

impl HarmCategory {
    fn from_wire_str(value: &str) -> Self {
        match value {
            "HARM_CATEGORY_UNSPECIFIED" => Self::Unspecified,
            "HARM_CATEGORY_DEROGATORY" => Self::Derogatory,
            "HARM_CATEGORY_TOXICITY" => Self::Toxicity,
            "HARM_CATEGORY_VIOLENCE" => Self::Violence,
            "HARM_CATEGORY_SEXUAL" => Self::Sexual,
            "HARM_CATEGORY_MEDICAL" => Self::Medical,
            "HARM_CATEGORY_DANGEROUS" => Self::Dangerous,
            "HARM_CATEGORY_HARASSMENT" => Self::Harassment,
            "HARM_CATEGORY_HATE_SPEECH" => Self::HateSpeech,
            "HARM_CATEGORY_SEXUALLY_EXPLICIT" => Self::SexuallyExplicit,
            "HARM_CATEGORY_DANGEROUS_CONTENT" => Self::DangerousContent,
            "HARM_CATEGORY_CIVIC_INTEGRITY" => Self::CivicIntegrity,
            "HARM_CATEGORY_JAILBREAK" => Self::Jailbreak,
            _ => Self::Unspecified,
        }
    }

    fn from_wire_number(value: i64) -> Self {
        match value {
            0 => Self::Unspecified,
            1 => Self::HateSpeech,
            2 => Self::DangerousContent,
            3 => Self::Harassment,
            4 => Self::SexuallyExplicit,
            5 => Self::CivicIntegrity,
            6 => Self::Jailbreak,
            _ => Self::Unspecified,
        }
    }
}

impl<'de> Deserialize<'de> for HarmCategory {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::String(s) => Ok(Self::from_wire_str(&s)),
            serde_json::Value::Number(n) => {
                n.as_i64().map(Self::from_wire_number).ok_or_else(|| {
                    de::Error::custom("harm category must be an integer-compatible number")
                })
            }
            _ => Err(de::Error::custom("harm category must be a string or integer")),
        }
    }
}

/// Threshold for blocking harmful content
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HarmBlockThreshold {
    /// Threshold is unspecified.
    HarmBlockThresholdUnspecified,
    /// Content with NEGLIGIBLE will be allowed.
    BlockLowAndAbove,
    /// Content with NEGLIGIBLE and LOW will be allowed.
    BlockMediumAndAbove,
    /// Content with NEGLIGIBLE, LOW, and MEDIUM will be allowed.
    BlockOnlyHigh,
    /// All content will be allowed.
    BlockNone,
    /// Turn off the safety filter.
    Off,
}

/// Probability that content is harmful
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HarmProbability {
    /// Probability is unspecified.
    HarmProbabilityUnspecified,
    /// Content has a negligible chance of being unsafe.
    Negligible,
    /// Content has a low chance of being unsafe.
    Low,
    /// Content has a medium chance of being unsafe.
    Medium,
    /// Content has a high chance of being unsafe.
    High,
}

impl HarmProbability {
    fn from_wire_str(value: &str) -> Self {
        match value {
            "HARM_PROBABILITY_UNSPECIFIED" => Self::HarmProbabilityUnspecified,
            "NEGLIGIBLE" => Self::Negligible,
            "LOW" => Self::Low,
            "MEDIUM" => Self::Medium,
            "HIGH" => Self::High,
            _ => Self::HarmProbabilityUnspecified,
        }
    }

    fn from_wire_number(value: i64) -> Self {
        match value {
            0 => Self::HarmProbabilityUnspecified,
            1 => Self::Negligible,
            2 => Self::Low,
            3 => Self::Medium,
            4 => Self::High,
            _ => Self::HarmProbabilityUnspecified,
        }
    }
}

impl<'de> Deserialize<'de> for HarmProbability {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::String(s) => Ok(Self::from_wire_str(&s)),
            serde_json::Value::Number(n) => {
                n.as_i64().map(Self::from_wire_number).ok_or_else(|| {
                    de::Error::custom("harm probability must be an integer-compatible number")
                })
            }
            _ => Err(de::Error::custom("harm probability must be a string or integer")),
        }
    }
}

/// Safety rating for content
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SafetyRating {
    /// The category of the safety rating
    pub category: HarmCategory,
    /// The probability that the content is harmful
    pub probability: HarmProbability,
}
