//! Server-Sent Events (SSE) types for streaming interactions.
//!
//! When `stream = true`, `POST /v1beta/interactions` returns an SSE stream of
//! [`InteractionSseEvent`] values rather than a single [`Interaction`]. The event
//! model replaces the legacy `content.*` events with a step-oriented model:
//!
//! - `interaction.created` — the interaction resource was created (in progress).
//! - `step.start` — a new step begins (carries the step shell).
//! - `step.delta` — incremental content for the in-flight step.
//! - `step.stop` — the current step is complete.
//! - `interaction.status_update` — a lifecycle status change.
//! - `interaction.completed` — the interaction finished (empty steps; accumulate
//!   deltas for the output).
//! - `error` — a terminal error occurred.

use super::model::{Interaction, InteractionStatus, Step};
use serde::{Deserialize, Serialize};

/// A streamed interaction event.
///
/// Discriminated on the `event_type` field. Unknown future event types
/// deserialize into [`InteractionSseEvent::Other`] rather than failing the stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum InteractionSseEvent {
    /// The interaction resource was created and is in progress.
    #[serde(rename = "interaction.created")]
    InteractionCreated {
        /// The newly created interaction (status `in_progress`).
        interaction: Interaction,
        /// Token for resuming the stream from this event.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        event_id: Option<String>,
    },
    /// The interaction completed. Steps are empty to reduce payload size —
    /// accumulate the preceding `step.delta` events for the output.
    #[serde(rename = "interaction.completed")]
    InteractionCompleted {
        /// The completed interaction (typically with empty steps).
        interaction: Interaction,
        /// Token for resuming the stream from this event.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        event_id: Option<String>,
    },
    /// A lifecycle status change for the interaction.
    #[serde(rename = "interaction.status_update")]
    InteractionStatusUpdate {
        /// The interaction ID this update applies to.
        interaction_id: String,
        /// The new status.
        status: InteractionStatus,
        /// Token for resuming the stream from this event.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        event_id: Option<String>,
    },
    /// A new step has begun. The `step` carries the step shell (e.g. an empty
    /// `model_output`); content arrives via subsequent `step.delta` events.
    #[serde(rename = "step.start")]
    StepStart {
        /// The index of the step in the timeline.
        index: i64,
        /// The step shell.
        step: Step,
        /// Token for resuming the stream from this event.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        event_id: Option<String>,
    },
    /// Incremental content for the in-flight step at `index`.
    #[serde(rename = "step.delta")]
    StepDelta {
        /// The index of the step this delta belongs to.
        index: i64,
        /// The incremental content fragment.
        delta: StepDelta,
        /// Token for resuming the stream from this event.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        event_id: Option<String>,
    },
    /// The step at `index` is complete.
    #[serde(rename = "step.stop")]
    StepStop {
        /// The index of the completed step.
        index: i64,
        /// Token for resuming the stream from this event.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        event_id: Option<String>,
    },
    /// A terminal error occurred during the interaction.
    Error {
        /// The error payload.
        error: InteractionStreamError,
        /// Token for resuming the stream from this event.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        event_id: Option<String>,
    },
    /// An event type not modelled by this crate version.
    #[serde(untagged)]
    Other(serde_json::Value),
}

impl InteractionSseEvent {
    /// Returns the incremental text fragment if this is a text `step.delta`.
    ///
    /// This is the most common accumulation path: collect `text_delta()` across
    /// the stream to build the final response text.
    pub fn text_delta(&self) -> Option<&str> {
        match self {
            InteractionSseEvent::StepDelta { delta: StepDelta::Text { text }, .. } => Some(text),
            _ => None,
        }
    }
}

/// The incremental payload carried by a `step.delta` event.
///
/// For text generation this is a `text` fragment. For streaming function calls,
/// `arguments_delta` carries a partial JSON string that must be accumulated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StepDelta {
    /// An incremental text fragment.
    Text {
        /// The text fragment to append.
        text: String,
    },
    /// An incremental function-call arguments fragment (partial JSON string).
    FunctionCall {
        /// Partial JSON string of arguments to accumulate.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        arguments_delta: Option<String>,
        /// The function name, present on the first delta.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// The call ID, present on the first delta.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    /// A delta type not modelled by this crate version.
    #[serde(untagged)]
    Other(serde_json::Value),
}

/// An error payload from an `error` SSE event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InteractionStreamError {
    /// A human-readable error message.
    #[serde(default)]
    pub message: String,
    /// A URI/code identifying the error type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}
