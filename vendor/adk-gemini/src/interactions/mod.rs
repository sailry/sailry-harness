//! Gemini Interactions API (Beta).
//!
//! The Interactions API is Google's new direction for the Gemini API. It replaces
//! the `generateContent` request/response shape with a stateful
//! [`Interaction`](crate::interactions::Interaction) resource built around a
//! typed [`Step`](crate::interactions::Step) timeline, polymorphic content
//! blocks, and an explicit streaming event model.
//!
//! Unlike `generateContent`, the Interactions API offers:
//!
//! - **Server-side history** via `previous_interaction_id` (no need to resend the
//!   full transcript each turn).
//! - **Observable execution steps** — thoughts, tool calls, tool results, and the
//!   final model output are all surfaced as typed
//!   [`Step`](crate::interactions::Step) entries.
//! - **Background and long-running tasks** via `background = true`.
//! - **Agentic workflows** with native multi-step tool use.
//!
//! This module is gated behind the `interactions` feature flag. It is additive and
//! does not change any existing `generateContent` API.
//!
//! # Quick start
//!
//! ```rust,ignore
//! use adk_gemini::{Gemini, Model};
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let gemini = Gemini::new("YOUR_API_KEY")?;
//!
//! let interaction = gemini
//!     .create_interaction()
//!     .model(Model::Gemini35Flash)
//!     .input_text("Explain agentic workflows in one sentence.")
//!     .send()
//!     .await?;
//!
//! println!("{}", interaction.output_text().unwrap_or_default());
//! # Ok(())
//! # }
//! ```
//!
//! # Status
//!
//! The Interactions API is in **beta**. Google may introduce breaking changes to
//! the request/response schema. The types here track the `Api-Revision:
//! 2026-05-20` schema (the `steps` schema with polymorphic `response_format`).

mod agent_config;
mod builder;
pub mod environment;
pub mod managed_agent;
mod model;
mod sse;
pub(crate) mod validate;

pub use agent_config::AgentConfig;
pub use builder::InteractionBuilder;
pub use environment::{
    Environment, EnvironmentConfig, EnvironmentSource, NetworkConfig, NetworkRule, TransformMap,
};
pub use managed_agent::{CreateAgentRequest, ListAgentsResponse, ManagedAgentBuilder, SavedAgent};
pub use model::{
    AudioContent, Content, CreateInteractionRequest, DocumentContent, GenerationConfig,
    ImageConfig, ImageContent, Input, Interaction, InteractionStatus, ModalityTokens,
    ResponseFormat, ResponseModality, ServiceTier, Step, TextContent, ThinkingSummaries, Tool,
    ToolChoice, Usage, VideoContent,
};
pub use sse::{InteractionSseEvent, InteractionStreamError, StepDelta};

/// The `Api-Revision` header value pinning the steps-schema contract this module
/// targets. Sent on every Interactions API request.
pub const API_REVISION: &str = "2026-05-20";
