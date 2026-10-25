use super::*;

impl View {
    pub(super) fn loading_history(&self) -> bool {
        self.session.is_some() && self.history.snapshot.is_none() && self.rows.is_empty()
    }
}
