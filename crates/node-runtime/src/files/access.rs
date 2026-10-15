//! Manual controller access is derived by admission, never a public request flag.
use sailry_protocol::{Fault, Request};
use std::path::Path;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Access {
    #[default]
    Restricted,
    Manual,
}

impl Access {
    pub(crate) fn request(request: &Request, internal: bool) -> Self {
        if !internal
            && request
                .plugin
                .as_ref()
                .is_none_or(|context| context.turn.is_none() && context.invocation.is_none())
        {
            Self::Manual
        } else {
            Self::Restricted
        }
    }

    pub(crate) fn scope(self, root: &Path) -> Scope<'_> {
        Scope { root, access: self }
    }
}

/// The registered root and its derived access travel through the same kernels.
#[derive(Clone, Copy)]
pub(crate) struct Scope<'a> {
    root: &'a Path,
    pub(crate) access: Access,
}

impl<'a> Scope<'a> {
    pub(crate) fn path(self) -> &'a Path {
        self.root
    }

    pub(crate) fn components(self, value: &str, allow_root: bool) -> Result<Vec<&str>, Fault> {
        super::path::components_with(value, allow_root, self.access)
    }

    pub(crate) fn entries(self, value: &str) -> Result<Vec<&str>, Fault> {
        super::path::entries_with(value, self.access)
    }
}

impl<'a, Root: AsRef<Path> + ?Sized> From<&'a Root> for Scope<'a> {
    fn from(root: &'a Root) -> Self {
        Access::Restricted.scope(root.as_ref())
    }
}

#[cfg(test)]
mod tests;
