//! One secondary summary request covers either layout repair or size recovery.
use super::*;

impl Summarizer {
    fn shortened(&self, request: &LlmRequest) -> adk_core::Result<LlmRequest> {
        let Some(context) = self.context else {
            return Err(invalid_at(
                Stage::Budget,
                Reason::NoFittingPrefix,
                "context summary capacity is unknown",
            ));
        };
        let output = self.summary_output();
        let input = context.saturating_sub(output);
        let revisions = model::history::Revisions::new();
        for percent in [85, 70, 50, 35] {
            let mut candidate = request.clone();
            model::history::mask(&mut candidate, 0, 0, &revisions)?;
            let window = model::history::Window {
                context: input.saturating_mul(percent) / 100 + output,
                output,
            };
            if model::history::limit(&mut candidate, Some(window), &revisions).is_ok()
                && self.fits_request(&candidate)?
            {
                return Ok(candidate);
            }
        }
        Err(invalid_at(
            Stage::Budget,
            Reason::NoFittingPrefix,
            "context summary input cannot fit without discarding immutable content",
        ))
    }

    pub(super) async fn response(&self, request: LlmRequest) -> adk_core::Result<Summary> {
        model::summary_scope(self.response_inner(request)).await
    }

    async fn response_inner(&self, mut request: LlmRequest) -> adk_core::Result<Summary> {
        if !self.fits_request(&request)? {
            request = self.shortened(&request)?;
        }
        let response = summarize(&*self.model, request.clone()).await;
        let response = match response {
            Ok(response)
                if !validation::headings(&response.text)
                    && !response.text.trim().is_empty()
                    && model::summary_retry() =>
            {
                let mut repair = request;
                repair
                    .contents
                    .push(Content::new("model").with_text(response.text));
                repair.contents.push(Content::new("user").with_text(
                    "Repair only the handoff layout. Preserve the same facts and language, add every required ## heading in the specified order, and return the complete concise handoff. Do not execute tools or continue the task."
                ));
                if !self.fits_request(&repair)? {
                    repair = self.shortened(&repair)?;
                }
                summarize(&*self.model, repair).await?
            }
            Ok(response) => response,
            Err(error) if failure::recoverable(&error) && model::summary_retry() => {
                let smaller = match self.shortened(&request) {
                    Ok(smaller) => smaller,
                    Err(_) => return Err(error),
                };
                // A provider budget error receives one changed, smaller request.
                // No retry is spent repeating an identical rejected envelope.
                if estimate::request(&smaller)? >= estimate::request(&request)? {
                    return Err(error);
                }
                summarize(&*self.model, smaller).await?
            }
            Err(error) => return Err(error),
        };
        if response.text.trim().is_empty() {
            return Err(invalid_at(
                Stage::Summary,
                Reason::EmptySummary,
                "context summary is empty",
            ));
        }
        if !validation::headings(&response.text) {
            return Err(invalid_at(
                Stage::Summary,
                Reason::UnexpectedOutput,
                "context summary is missing required headings",
            ));
        }
        Ok(response)
    }
}

#[cfg(test)]
mod tests;
