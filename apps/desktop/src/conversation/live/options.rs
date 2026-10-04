//! Panel-owned composer choices; input, overlays, and dispatch stay shared.
use sailry_protocol::Permission;

#[derive(Clone, Copy, Default)]
pub(crate) enum Mentions {
    #[default]
    Workspace,
    Database,
    Attachments,
}

#[derive(Clone, Copy)]
pub(crate) struct ComposerOptions {
    pub mentions: Mentions,
    pub prompts: &'static [&'static str],
    pub permissions: &'static [Permission],
    pub skills: bool,
}

impl Default for ComposerOptions {
    fn default() -> Self {
        Self {
            mentions: Mentions::Workspace,
            prompts: &["review", "test", "plan", "explain"],
            permissions: &super::super::permission::MODES,
            skills: true,
        }
    }
}

impl ComposerOptions {
    pub(crate) fn connection(mentions: Mentions) -> Self {
        Self {
            mentions,
            prompts: &["plan", "explain"],
            permissions: &[Permission::Ask, Permission::Full],
            skills: false,
        }
    }
}
