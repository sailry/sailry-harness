//! Initialization state is independent of the selected package identity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::plugins) enum Initialization {
    #[default]
    Unloaded,
    Loading,
    Published,
    Failed,
    Invalidated,
}

impl Initialization {
    pub(in crate::plugins) fn failed(self) -> bool {
        self == Self::Failed
    }

    pub(in crate::plugins) fn loading(self) -> bool {
        self == Self::Loading
    }
}
