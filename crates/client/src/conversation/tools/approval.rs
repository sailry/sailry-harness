//! Controllers choose layout and localization for the same approval facts.
use super::Call;
use sailry_protocol::{conversation::Page, process::Access, tool::LocalText};
use serde::Serialize;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ApprovalView {
    pub prompt: Option<LocalText>,
    pub context: Option<String>,
    pub description: Option<LocalText>,
    pub unrestricted: bool,
    pub notices: Vec<LocalText>,
    pub code: bool,
}

impl Call {
    pub fn approval_view(&self, page: &Page) -> ApprovalView {
        let unrestricted = self
            .approval
            .as_ref()
            .is_some_and(|approval| approval.access == Some(Access::Unrestricted));
        let Some(display) = self.summary(page).and_then(|_| self.resolved()) else {
            return ApprovalView {
                unrestricted,
                ..Default::default()
            };
        };
        let description = display.approval.first().cloned().or_else(|| {
            display
                .input
                .target
                .as_ref()
                .or(display.input.summary.as_ref())
                .map(|field| LocalText {
                    label: field.text.clone(),
                    locales: Default::default(),
                })
        });
        let code = display.input.summary.as_ref().is_some_and(|field| {
            field.code
                && description
                    .as_ref()
                    .is_some_and(|description| description.label == field.text)
        });
        ApprovalView {
            prompt: description.clone().or_else(|| display.label.clone()),
            context: display
                .input
                .context
                .as_ref()
                .map(|field| field.text.clone()),
            notices: display
                .approval
                .iter()
                .filter(|notice| Some(*notice) != description.as_ref())
                .cloned()
                .collect(),
            description,
            unrestricted,
            code,
        }
    }
}
