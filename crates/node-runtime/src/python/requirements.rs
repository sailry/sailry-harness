use sailry_protocol::{Fault, plugin::Reference};
use sha2::{Digest, Sha256};

#[derive(Clone)]
pub(crate) struct Requirements {
    pub(super) package: Reference,
    pub(super) content: String,
}

impl Requirements {
    /// The caller supplies bytes from the immutable package captured by admission.
    pub(crate) fn new(package: Reference, content: String) -> Result<Self, Fault> {
        if package.name.is_empty()
            || package.digest.len() != 64
            || !package.digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            || content.len() > 512 * 1024
            || content.contains('\0')
        {
            return Err(Fault::invalid("invalid captured Python requirements"));
        }
        // Hash-locked pip files may contain pins, markers and hash continuations.
        // Includes or installer options cannot override this owner's wheel-only,
        // isolated index policy or read files outside the captured requirements.
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('-') && !line.starts_with("--hash=sha256:") {
                return Err(Fault::invalid(
                    "Python requirements cannot contain installer options or file includes",
                ));
            }
            if !line.starts_with('#')
                && line.split_whitespace().any(|part| {
                    part.starts_with("--") && !part.starts_with("--hash=sha256:")
                        || part.starts_with("file:")
                        || part.starts_with("git+")
                })
            {
                return Err(Fault::invalid(
                    "Python requirements must use hashed, prebuilt packages",
                ));
            }
        }
        Ok(Self { package, content })
    }

    pub(super) fn key(&self, runtime: &str) -> String {
        let mut digest = Sha256::new();
        digest.update(runtime.as_bytes());
        digest.update(Sha256::digest(self.content.as_bytes()));
        format!("{:x}", digest.finalize())
    }
}
