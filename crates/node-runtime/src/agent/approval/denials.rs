//! Only results after this invocation's unchanged input receive denial explanations.
use super::*;
use adk_core::{AdkError, BeforeModelCallback, BeforeModelResult, Content};
use std::sync::Mutex;

struct Fence {
    input: Content,
    index: Option<usize>,
}

impl Fence {
    fn locate(&mut self, contents: &[Content]) -> adk_core::Result<usize> {
        let index = match self.index {
            Some(index) => index,
            None => contents
                .iter()
                .rposition(|content| content.role == "user")
                .ok_or_else(|| {
                    AdkError::agent("current turn input is missing from model history")
                })?,
        };
        if contents.get(index).is_none_or(|content| {
            content.role != self.input.role || content.parts != self.input.parts
        }) {
            return Err(AdkError::agent(
                "current turn input does not identify model history",
            ));
        }
        self.index = Some(index);
        Ok(index)
    }
}

pub(in crate::agent) fn bind(
    ingress: &Arc<Ingress>,
    turn: TurnId,
    input: Content,
) -> BeforeModelCallback {
    let ingress = ingress.clone();
    let fence = Arc::new(Mutex::new(Fence { input, index: None }));
    Box::new(move |_, mut request| {
        let ingress = ingress.clone();
        let fence = fence.clone();
        Box::pin(async move {
            let index = fence
                .lock()
                .map_err(|_| AdkError::agent("current turn history fence is unavailable"))?
                .locate(&request.contents)?;
            if request.contents[index + 1..].iter().any(|content| {
                content
                    .parts
                    .iter()
                    .any(|part| matches!(part, adk_core::Part::FunctionResponse { .. }))
            }) {
                let denials = ingress.denials(turn).await.map_err(|error| {
                    AdkError::agent("denial explanations could not be read").with_source(error)
                })?;
                for content in &mut request.contents[index + 1..] {
                    crate::store::agent::enrich_denials(content, &denials);
                }
            }
            Ok(BeforeModelResult::Continue(request))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_repeated_historical_inputs() {
        let input = Content::new("user").with_text("Repeat");
        let mut fence = Fence {
            input: input.clone(),
            index: None,
        };
        let mut contents = vec![input.clone(), Content::new("function"), input];
        assert_eq!(fence.locate(&contents).unwrap(), 2);
        contents.push(Content::new("function"));
        assert_eq!(fence.locate(&contents).unwrap(), 2);
    }

    #[test]
    fn rejects_shifted_or_changed_input() {
        let input = Content::new("user").with_text("Current");
        let mut fence = Fence {
            input: input.clone(),
            index: None,
        };
        assert!(
            fence
                .locate(&[Content::new("user").with_text("Other")])
                .is_err()
        );
        assert_eq!(
            fence
                .locate(&[Content::new("model"), input.clone()])
                .unwrap(),
            1
        );
        assert!(fence.locate(&[input]).is_err());
    }
}
