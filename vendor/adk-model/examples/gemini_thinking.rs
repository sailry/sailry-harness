//! Gemini Thinking Configuration Examples
//!
//! Demonstrates how to configure thinking levels and budgets across
//! different Gemini model generations:
//!
//! - **Gemini 3 series** — level-based: Minimal, Low, Medium, High
//! - **Gemini 2.5 series** — budget-based: token count (0 to 32768)
//! - **Older models** — no thinking config needed (ignored gracefully)
//!
//! Requires `GEMINI_API_KEY` or `GOOGLE_API_KEY` environment variable.
//!
//! ```bash
//! cargo run -p adk-model --features gemini --example gemini_thinking
//! ```

use adk_core::{Content, Llm, LlmRequest, Part};
use adk_model::gemini::{GeminiModel, ThinkingConfig, ThinkingLevel};
use futures::StreamExt;

const PROMPT: &str = "What is 17 * 23? Show your work.";

fn api_key() -> String {
    std::env::var("GEMINI_API_KEY")
        .or_else(|_| std::env::var("GOOGLE_API_KEY"))
        .expect("GEMINI_API_KEY or GOOGLE_API_KEY must be set")
}

async fn run_model(label: &str, model: &GeminiModel) {
    println!("--- {label} ---");
    println!("    Model: {}", model.name());
    if let Some(tc) = model.thinking_config() {
        println!("    Thinking: {tc:?}");
    } else {
        println!("    Thinking: None (default)");
    }
    println!();

    let request = LlmRequest {
        model: String::new(),
        contents: vec![Content::new("user").with_text(PROMPT)],
        config: None,
        tools: Default::default(),
        previous_response_id: None,
    };

    let start = std::time::Instant::now();
    match model.generate_content(request, false).await {
        Ok(mut stream) => {
            while let Some(result) = stream.next().await {
                match result {
                    Ok(response) => {
                        if response.partial {
                            continue;
                        }
                        if let Some(content) = &response.content {
                            for part in &content.parts {
                                match part {
                                    Part::Thinking { thinking, .. } => {
                                        let preview = if thinking.len() > 120 {
                                            let mut end = 120;
                                            while end > 0 && !thinking.is_char_boundary(end) {
                                                end -= 1;
                                            }
                                            format!("{}...", &thinking[..end])
                                        } else {
                                            thinking.clone()
                                        };
                                        println!("    💭 Thinking: {preview}");
                                    }
                                    Part::Text { text } if !text.trim().is_empty() => {
                                        println!("    📝 Response: {}", text.trim());
                                    }
                                    _ => {}
                                }
                            }
                        }
                        if let Some(usage) = &response.usage_metadata {
                            println!(
                                "    📊 Tokens: {} prompt, {} output, {} total{}",
                                usage.prompt_token_count,
                                usage.candidates_token_count,
                                usage.total_token_count,
                                usage
                                    .thinking_token_count
                                    .map(|t| format!(", {t} thinking"))
                                    .unwrap_or_default()
                            );
                        }
                    }
                    Err(e) => {
                        println!("    ❌ Error: {e}");
                    }
                }
            }
        }
        Err(e) => {
            println!("    ❌ Error: {e}");
        }
    }

    println!("    ⏱️  Latency: {:.1}s", start.elapsed().as_secs_f64());
    println!();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let key = api_key();

    println!("=== Gemini Thinking Configuration Examples ===\n");
    println!("Prompt: \"{PROMPT}\"\n");

    // ---------------------------------------------------------------
    // 1. Gemini 2.5 Flash — no thinking config (default behavior)
    // ---------------------------------------------------------------
    let model = GeminiModel::new(&key, "gemini-2.5-flash")?;
    run_model("Gemini 2.5 Flash — default (no thinking config)", &model).await;

    // ---------------------------------------------------------------
    // 2. Gemini 2.5 Flash — budget-based thinking (2048 tokens)
    // ---------------------------------------------------------------
    let model = GeminiModel::new(&key, "gemini-2.5-flash")?
        .with_thinking_config(ThinkingConfig::new().with_thinking_budget(2048));
    run_model("Gemini 2.5 Flash — budget: 2048 tokens", &model).await;

    // ---------------------------------------------------------------
    // 3. Gemini 2.5 Flash — thinking disabled (budget = 0)
    // ---------------------------------------------------------------
    let model = GeminiModel::new(&key, "gemini-2.5-flash")?
        .with_thinking_config(ThinkingConfig::new().with_thinking_budget(0));
    run_model("Gemini 2.5 Flash — thinking disabled (budget=0)", &model).await;

    // ---------------------------------------------------------------
    // 4. Gemini 2.5 Flash — dynamic thinking (model decides)
    // ---------------------------------------------------------------
    let model = GeminiModel::new(&key, "gemini-2.5-flash")?
        .with_thinking_config(ThinkingConfig::dynamic_thinking());
    run_model("Gemini 2.5 Flash — dynamic thinking", &model).await;

    // ---------------------------------------------------------------
    // 5. Gemini 3 Flash — level-based: Low (fast, cheap)
    // ---------------------------------------------------------------
    let model = GeminiModel::new(&key, "gemini-3-flash-preview")?.with_thinking_config(
        ThinkingConfig::new().with_thinking_level(ThinkingLevel::Low).with_thoughts_included(true),
    );
    run_model("Gemini 3 Flash — ThinkingLevel::Low", &model).await;

    // ---------------------------------------------------------------
    // 6. Gemini 3 Flash — level-based: High (deep reasoning)
    // ---------------------------------------------------------------
    let model = GeminiModel::new(&key, "gemini-3-flash-preview")?.with_thinking_config(
        ThinkingConfig::new().with_thinking_level(ThinkingLevel::High).with_thoughts_included(true),
    );
    run_model("Gemini 3 Flash — ThinkingLevel::High", &model).await;

    // ---------------------------------------------------------------
    // 7. Backward compatibility — older model without thinking
    //    ThinkingConfig is set but the model ignores it gracefully.
    // ---------------------------------------------------------------
    let model = GeminiModel::new(&key, "gemini-2.5-flash")?;
    // No thinking config — works the same as before the feature was added
    run_model("Backward compat — no thinking config (same as pre-0.7)", &model).await;

    // ---------------------------------------------------------------
    // 8. Runtime reconfiguration via set_thinking_config
    // ---------------------------------------------------------------
    let mut model = GeminiModel::new(&key, "gemini-2.5-flash")?;
    model.set_thinking_config(ThinkingConfig::new().with_thinking_budget(4096));
    run_model("Runtime reconfig — set_thinking_config(budget=4096)", &model).await;

    println!("=== All examples complete ===");
    Ok(())
}
