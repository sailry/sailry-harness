use super::*;

impl View {
    pub(crate) fn tool_history(&self) -> &History {
        &self.history
    }

    pub(crate) fn embedded(
        binding: Binding,
        session: Option<Session>,
        options: ComposerOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self::new(binding, session, window, cx);
        view.sidebar = true;
        view.composer_options = options;
        view.git = false;
        view
    }

    pub(crate) fn for_connection(
        binding: Binding,
        session: Option<Session>,
        resource: sailry_protocol::connection::Resource,
        options: ComposerOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut binding = binding;
        binding.project = None;
        binding.worktree = session.as_ref().map(|session| session.worktree);
        binding.project_name = SharedString::default();
        binding.branch = SharedString::default();
        let mut view = Self::embedded(binding, session, options, window, cx);
        view.draft_permission = Some(sailry_protocol::Permission::Ask);
        view.resource = Some(resource);
        view
    }

    pub(crate) fn for_assistant(
        binding: Binding,
        session: Option<Session>,
        assistant: sailry_protocol::plugin::conversation::Binding,
        resource: Option<sailry_protocol::connection::Resource>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut binding = binding;
        if binding.project.is_none() {
            binding.worktree = session.as_ref().map(|session| session.worktree);
        }
        let options = match resource {
            Some(sailry_protocol::connection::Resource::Database(_)) => {
                ComposerOptions::connection(Mentions::Database)
            }
            Some(sailry_protocol::connection::Resource::Ssh(_)) => {
                ComposerOptions::connection(Mentions::Attachments)
            }
            None if binding.project.is_some() => ComposerOptions::default(),
            None => ComposerOptions::connection(Mentions::Attachments),
        };
        let mut view = Self::embedded(binding, session, options, window, cx);
        view.assistant = Some(assistant);
        view.resource = resource;
        view.draft_permission = Some(sailry_protocol::Permission::Ask);
        view
    }
}
