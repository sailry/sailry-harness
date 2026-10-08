//! Fluent builder for creating interactions.

use std::sync::Arc;

use crate::backend::BackendStream;
use crate::{
    ThinkingLevel,
    client::{Error, GeminiClient, Model},
    interactions::agent_config::AgentConfig,
    interactions::environment::Environment,
    interactions::model::{
        Content, CreateInteractionRequest, GenerationConfig, Input, ResponseFormat,
        ResponseModality, ServiceTier, Step, ThinkingSummaries, Tool, ToolChoice,
    },
    interactions::sse::InteractionSseEvent,
    interactions::validate::validate_interaction_request,
};

/// A fluent builder for `POST /v1beta/interactions`.
///
/// Build a request with the chaining methods, then call [`send`](Self::send) for
/// a single [`Interaction`](crate::interactions::Interaction) or
/// [`stream`](Self::stream) for an SSE event stream.
///
/// # Example
///
/// ```rust,ignore
/// let interaction = gemini
///     .create_interaction()
///     .model(Model::Gemini35Flash)
///     .system_instruction("You are concise.")
///     .input_text("What is the capital of France?")
///     .thinking_level(ThinkingLevel::Low)
///     .send()
///     .await?;
/// println!("{}", interaction.output_text().unwrap_or_default());
/// ```
pub struct InteractionBuilder {
    client: Arc<GeminiClient>,
    request: CreateInteractionRequest,
    /// Accumulated content blocks for the input, when building incrementally.
    content_parts: Vec<Content>,
    /// Accumulated steps for the input, when supplying stateless history.
    steps: Vec<Step>,
    /// A direct text prompt, when set via [`input_text`](Self::input_text).
    text_input: Option<String>,
}

impl InteractionBuilder {
    /// Create a new builder bound to a client.
    pub(crate) fn new(client: Arc<GeminiClient>) -> Self {
        Self {
            client,
            request: CreateInteractionRequest::default(),
            content_parts: Vec::new(),
            steps: Vec::new(),
            text_input: None,
        }
    }

    /// Set the model to use (mutually exclusive with [`agent`](Self::agent)).
    pub fn model(mut self, model: impl Into<Model>) -> Self {
        let model = model.into();
        // The Interactions API expects bare model IDs (no "models/" prefix).
        let id = model.as_str().strip_prefix("models/").unwrap_or(model.as_str()).to_string();
        self.request.model = Some(id);
        self
    }

    /// Set the agent to use, e.g. `"deep-research-pro-preview-12-2025"`
    /// (mutually exclusive with [`model`](Self::model)).
    pub fn agent(mut self, agent: impl Into<String>) -> Self {
        self.request.agent = Some(agent.into());
        self
    }

    /// Set a bare text prompt as the interaction input.
    ///
    /// Overrides any content blocks or steps added separately.
    pub fn input_text(mut self, text: impl Into<String>) -> Self {
        self.text_input = Some(text.into());
        self
    }

    /// Append a content block to the interaction input (single turn).
    pub fn content(mut self, content: Content) -> Self {
        self.content_parts.push(content);
        self
    }

    /// Append an inline (base64) image to the interaction input.
    pub fn image(mut self, data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        self.content_parts.push(Content::image(data, mime_type));
        self
    }

    /// Supply the full step history (stateless multi-turn conversation).
    ///
    /// Prefer [`previous_interaction_id`](Self::previous_interaction_id) for
    /// server-side history; use this only when running stateless.
    pub fn steps(mut self, steps: Vec<Step>) -> Self {
        self.steps = steps;
        self
    }

    /// Append a single step to the input history.
    pub fn step(mut self, step: Step) -> Self {
        self.steps.push(step);
        self
    }

    /// Append a `function_result` step answering a pending client-side tool call.
    ///
    /// Use the `call_id` from [`Interaction::pending_function_calls`](crate::interactions::Interaction::pending_function_calls).
    pub fn function_result(
        mut self,
        call_id: impl Into<String>,
        name: impl Into<String>,
        result: serde_json::Value,
    ) -> Self {
        self.steps.push(Step::FunctionResult {
            call_id: call_id.into(),
            name: Some(name.into()),
            result,
            is_error: None,
            signature: None,
        });
        self
    }

    /// Set the system instruction for the interaction.
    pub fn system_instruction(mut self, text: impl Into<String>) -> Self {
        self.request.system_instruction = Some(text.into());
        self
    }

    /// Add a tool declaration.
    pub fn tool(mut self, tool: Tool) -> Self {
        self.request.tools.push(tool);
        self
    }

    /// Add a client-side function tool declaration.
    pub fn function(
        self,
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        self.tool(Tool::function(name, description, parameters))
    }

    /// Continue a stored conversation via server-side history.
    ///
    /// Pass the `id` of a prior [`Interaction`](crate::interactions::Interaction).
    /// Note: `tools`, `system_instruction`, and `generation_config` are
    /// interaction-scoped and must be re-specified each turn.
    pub fn previous_interaction_id(mut self, id: impl Into<String>) -> Self {
        self.request.previous_interaction_id = Some(id.into());
        self
    }

    /// Run the interaction in the background (long-running tasks).
    ///
    /// Incompatible with `store = false`.
    pub fn background(mut self, background: bool) -> Self {
        self.request.background = Some(background);
        self
    }

    /// Control whether the interaction is stored server-side (default: true).
    ///
    /// Setting `false` opts out of storage and disables
    /// `previous_interaction_id` continuation and background execution.
    pub fn store(mut self, store: bool) -> Self {
        self.request.store = Some(store);
        self
    }

    /// Set the reasoning effort level.
    pub fn thinking_level(mut self, level: ThinkingLevel) -> Self {
        self.generation_config_mut().thinking_level = Some(level);
        self
    }

    /// Control whether thought summaries are included in the response.
    pub fn thinking_summaries(mut self, summaries: ThinkingSummaries) -> Self {
        self.generation_config_mut().thinking_summaries = Some(summaries);
        self
    }

    /// Set the maximum number of output tokens.
    pub fn max_output_tokens(mut self, max: i32) -> Self {
        self.generation_config_mut().max_output_tokens = Some(max);
        self
    }

    /// Set the sampling temperature. Discouraged for Gemini 3.x models.
    pub fn temperature(mut self, temperature: f32) -> Self {
        self.generation_config_mut().temperature = Some(temperature);
        self
    }

    /// Set the tool-choice configuration.
    pub fn tool_choice(mut self, choice: ToolChoice) -> Self {
        self.generation_config_mut().tool_choice = Some(choice);
        self
    }

    /// Replace the entire generation configuration.
    pub fn generation_config(mut self, config: GenerationConfig) -> Self {
        self.request.generation_config = Some(config);
        self
    }

    /// Constrain the output to a single response format (e.g. JSON schema).
    pub fn response_format(mut self, format: ResponseFormat) -> Self {
        self.request.response_format = Some(format);
        self
    }

    /// Request structured JSON output conforming to a schema.
    pub fn json_schema(mut self, schema: serde_json::Value) -> Self {
        self.request.response_format = Some(ResponseFormat::json_schema(schema));
        self
    }

    /// Request multiple output modalities (e.g. text + audio).
    pub fn response_modalities(mut self, modalities: Vec<ResponseModality>) -> Self {
        self.request.response_modalities = modalities;
        self
    }

    /// Set the service tier.
    pub fn service_tier(mut self, tier: ServiceTier) -> Self {
        self.request.service_tier = Some(tier);
        self
    }

    /// Attach an environment to the interaction.
    ///
    /// An environment is a server-side sandbox workspace. You can request a fresh
    /// remote sandbox, resume an existing one by ID, or provide an inline
    /// configuration with sources and network rules.
    ///
    /// This is a direct-client capability and is not wired into the `adk-runner`
    /// `Agent` trait.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use adk_gemini::interactions::Environment;
    ///
    /// // Fresh remote sandbox
    /// let interaction = gemini
    ///     .create_interaction()
    ///     .antigravity()
    ///     .environment(Environment::remote())
    ///     .input_text("Fix the failing test in src/lib.rs")
    ///     .send()
    ///     .await?;
    ///
    /// // Resume an existing environment
    /// let interaction = gemini
    ///     .create_interaction()
    ///     .antigravity()
    ///     .environment(Environment::resume("env_abc123"))
    ///     .input_text("Now run the tests")
    ///     .send()
    ///     .await?;
    /// ```
    pub fn environment(mut self, env: Environment) -> Self {
        self.request.environment = Some(env);
        self
    }

    /// Attach a managed-agent configuration to the interaction.
    ///
    /// Currently only Deep Research uses an agent config; Antigravity takes none.
    ///
    /// This is a direct-client capability and is not wired into the `adk-runner`
    /// `Agent` trait.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use adk_gemini::interactions::AgentConfig;
    ///
    /// let interaction = gemini
    ///     .create_interaction()
    ///     .deep_research("deep-research-preview-04-2026")
    ///     .agent_config(AgentConfig::deep_research())
    ///     .input_text("Research the state of WebAssembly in 2026")
    ///     .send()
    ///     .await?;
    /// ```
    pub fn agent_config(mut self, config: AgentConfig) -> Self {
        self.request.agent_config = Some(config);
        self
    }

    /// Convenience: configure for Antigravity.
    ///
    /// Sets `agent = "antigravity-preview-05-2026"`, clears `model`, and sets
    /// `store = true`. Does **not** set `background` (Antigravity forbids it).
    ///
    /// This is a direct-client capability and is not wired into the `adk-runner`
    /// `Agent` trait.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use adk_gemini::interactions::Environment;
    ///
    /// let interaction = gemini
    ///     .create_interaction()
    ///     .antigravity()
    ///     .environment(Environment::remote())
    ///     .input_text("Fix the failing test in src/lib.rs")
    ///     .send()
    ///     .await?;
    /// ```
    pub fn antigravity(mut self) -> Self {
        self.request.agent = Some("antigravity-preview-05-2026".to_string());
        self.request.model = None;
        self.request.store = Some(true);
        self
    }

    /// Convenience: configure for Deep Research.
    ///
    /// Sets `agent` to the given ID, clears `model`, and sets `background = true`
    /// and `store = true`. Deep Research requires background execution.
    ///
    /// This is a direct-client capability and is not wired into the `adk-runner`
    /// `Agent` trait.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use adk_gemini::interactions::AgentConfig;
    ///
    /// let interaction = gemini
    ///     .create_interaction()
    ///     .deep_research("deep-research-preview-04-2026")
    ///     .agent_config(AgentConfig::deep_research())
    ///     .input_text("Research the state of WebAssembly in 2026")
    ///     .send()
    ///     .await?;
    /// ```
    pub fn deep_research(mut self, agent_id: impl Into<String>) -> Self {
        self.request.agent = Some(agent_id.into());
        self.request.model = None;
        self.request.background = Some(true);
        self.request.store = Some(true);
        self
    }

    fn generation_config_mut(&mut self) -> &mut GenerationConfig {
        self.request.generation_config.get_or_insert_with(GenerationConfig::default)
    }

    /// Finalize the input field from whatever was supplied.
    fn finalize_input(&mut self) {
        if let Some(text) = self.text_input.take() {
            self.request.input = Input::Text(text);
        } else if !self.steps.is_empty() {
            self.request.input = Input::Steps(std::mem::take(&mut self.steps));
        } else {
            self.request.input = Input::Content(std::mem::take(&mut self.content_parts));
        }
    }

    /// Send the request and return the resulting [`Interaction`](crate::interactions::Interaction).
    pub async fn send(mut self) -> Result<crate::interactions::Interaction, Error> {
        self.finalize_input();
        validate_interaction_request(&self.request)?;
        self.client.create_interaction(self.request).await
    }

    /// Send the request and return a stream of [`InteractionSseEvent`] values.
    pub async fn stream(mut self) -> Result<BackendStream<InteractionSseEvent>, Error> {
        self.finalize_input();
        validate_interaction_request(&self.request)?;
        self.request.stream = Some(true);
        self.client.create_interaction_stream(self.request).await
    }
}
