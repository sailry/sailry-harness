//! Selects only pinned CPU plugins through b11756's exported scoring ABI.
use super::{Error, Path, PathBuf, Result, library};

pub(super) fn variant(name: &str) -> bool {
    (name.starts_with("libggml-cpu-") && name.ends_with(".so"))
        || (name.starts_with("ggml-cpu-") && name.ends_with(".dll"))
}

pub(super) fn select(directory: &Path, variants: &[String]) -> Result<PathBuf> {
    let name = best(variants, |name| {
        let library = library(&directory.join(name))?;
        // SAFETY: b11756 exports int ggml_backend_score(void). Its detector is
        // compiled without the plugin's ISA flags, as in the official loader.
        let score = unsafe {
            library.get::<unsafe extern "C" fn() -> libc::c_int>(b"ggml_backend_score\0")
        }?;
        // SAFETY: the verified plugin stays loaded until its detector returns.
        Ok(unsafe { score() })
    })?;
    super::ordinary(&directory.join(name))
}

fn best(variants: &[String], mut score: impl FnMut(&str) -> Result<i32>) -> Result<&str> {
    let mut selected = None;
    let mut maximum = 0;
    let mut failed = None;
    for name in variants {
        match score(name) {
            Ok(value) if value > maximum => {
                maximum = value;
                selected = Some(name.as_str());
            }
            Err(error) => failed = Some(error),
            _ => {}
        }
    }
    selected.ok_or_else(|| {
        failed.unwrap_or(Error::Unavailable("no compatible CPU backend is available"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scores_only_locked_cpu_variants() {
        let names = [
            "libggml-cpu-x64.so",
            "libggml-cpu-haswell.so",
            "ggml-cpu-zen4.dll",
            "libggml-cuda.so",
            "llama.dll",
            "../libggml-cpu-external.so",
        ];
        let variants = names
            .into_iter()
            .filter(|name| variant(name))
            .map(String::from)
            .collect::<Vec<_>>();
        let selected = best(&variants, |name| {
            Ok(match name {
                "libggml-cpu-x64.so" => 1,
                "libggml-cpu-haswell.so" => 8,
                "ggml-cpu-zen4.dll" => 0,
                _ => panic!("unlocked backend was scored"),
            })
        })
        .unwrap();
        assert_eq!(selected, "libggml-cpu-haswell.so");
    }

    #[test]
    fn skips_unsupported_or_unloadable_variants() {
        let variants = ["unsupported", "unloadable", "portable"].map(String::from);
        assert_eq!(
            best(&variants, |name| match name {
                "unsupported" => Ok(0),
                "unloadable" => Err(Error::Unavailable("dependency is missing")),
                _ => Ok(1),
            })
            .unwrap(),
            "portable"
        );
        assert!(matches!(
            best(&variants[..1], |_| Ok(0)),
            Err(Error::Unavailable("no compatible CPU backend is available"))
        ));
        assert!(matches!(
            best(&variants[1..2], |_| Err(Error::Unavailable(
                "dependency is missing"
            ))),
            Err(Error::Unavailable("dependency is missing"))
        ));
    }
}
