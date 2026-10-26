use super::{Call, State};
use sailry_protocol::{
    conversation::{ApprovalState, Page},
    tool::Presentation,
};

impl Call {
    pub fn diagnostics(&self, page: &Page) -> bool {
        self.summary(page)
            .is_some_and(|summary| summary.diagnostics)
    }

    pub fn hidden(&self, page: &Page) -> bool {
        self.presentation == Presentation::Hidden
            && self.state == State::Returned
            && self.approval.is_none()
            && self.question.is_none()
            && !self.terminal_interaction
            && !self.diagnostics(page)
            && self.images(page).is_empty()
    }

    pub fn details(&self, page: &Page) -> bool {
        !self
            .approval
            .as_ref()
            .is_some_and(|approval| approval.state == ApprovalState::Denied)
            && self
                .summary(page)
                .is_some_and(|summary| summary.has_details)
    }

    pub fn raw_result(&self, page: &Page) -> bool {
        self.response.is_some() && self.summary(page).is_some_and(|summary| summary.raw_result)
    }
}
