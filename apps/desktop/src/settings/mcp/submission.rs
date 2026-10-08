//! Shared admission, retry identity and diagnostics for MCP drafts.
use super::*;
use sailry_protocol::Fault;

#[derive(Default)]
pub(super) struct Submission {
    pub(super) pending: bool,
    pub(super) request: Option<Request>,
    pub(super) error: Option<&'static str>,
    pub(super) detail: Option<String>,
    pub(super) details: bool,
}

impl Submission {
    pub(super) fn begin(
        &mut self,
        binding: &Binding,
        command: Option<Result<Command, &'static str>>,
    ) -> Option<tokio::task::JoinHandle<Result<Output, Fault>>> {
        if self.request.is_none() {
            match command.expect("a new submission requires a prepared command") {
                Ok(command) => self.request = Some(binding.client.prepare(command)),
                Err(error) => {
                    self.error = Some(error);
                    return None;
                }
            }
        }
        let request = self.request.clone().expect("MCP request is prepared");
        let binding = binding.clone();
        self.pending = true;
        self.error = None;
        self.detail = None;
        Some(
            binding
                .runtime
                .spawn(async move { binding.client.execute(request).await }),
        )
    }

    pub(super) fn failure(&mut self, error: Fault, key: &'static str) {
        if !plugins::live::uncertain(&error) {
            self.request = None;
        }
        self.error = Some(key);
        self.detail = Some(error.message);
    }

    pub(super) fn diagnostics(&self) -> Div {
        v_flex().w_full().gap_2().when(self.details, |view| {
            view.children(
                self.detail
                    .clone()
                    .map(|text| div().text_xs().max_h_16().overflow_hidden().child(text)),
            )
        })
    }

    pub(super) fn save_button(&self, id: &'static str, loading: bool, loaded: bool) -> Button {
        Button::new(id)
            .primary()
            .debug_selector(move || id.into())
            .loading(self.pending)
            .disabled(self.pending || loading || !loaded)
            .label(tr(if self.request.is_some() {
                "plugins_retry"
            } else {
                "settings_save"
            }))
    }
}
