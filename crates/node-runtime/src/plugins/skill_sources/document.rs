//! Document inspection reuses immutable acquisition and never installs or executes a skill.
//! The returned content is the Markdown body; the parsed frontmatter travels as `skill`.
use super::*;
use sailry_protocol::plugin::skills::{Document, DocumentSource};

impl Host {
    pub(crate) async fn read_skill_document(
        &self,
        source: &DocumentSource,
        stop: CancellationToken,
    ) -> Result<Document, Fault> {
        let _permit = self.read_permit(&stop).await?;
        match source {
            DocumentSource::Bundled { package, skill } => {
                if stop.is_cancelled() {
                    return Err(cancelled());
                }
                super::super::bundled::document(package, skill)
            }
            DocumentSource::Registry(source) => self.registry_document(source, stop).await,
            DocumentSource::Git { source, path } => {
                let selected = source::Selection::resolved(source, path)?;
                let (file, _) = self
                    .source_archive(&selected, Some(&source.commit), stop.clone(), None)
                    .await?;
                let path = path.clone();
                tokio::task::spawn_blocking(move || archive(file, &path, &stop))
                    .await
                    .map_err(|_| unavailable())?
            }
        }
    }
}

pub(in crate::plugins) fn archive(
    file: File,
    path: &str,
    stop: &CancellationToken,
) -> Result<Document, Fault> {
    let (_temporary, root, links) = super::super::archive::unpack_source(file, stop, 10_000)?;
    let root = super::super::sources::package_root(&root, path, &links)?;
    let directory = crate::files::path::root(&root)?;
    let bytes =
        super::super::package::read(&directory, "SKILL.md", super::super::skills::MAX_BYTES)?
            .ok_or_else(|| Fault::new(ErrorCode::NotFound, "selected directory has no SKILL.md"))?;
    if stop.is_cancelled() {
        return Err(cancelled());
    }
    parse(&bytes)
}

pub(in crate::plugins) fn parse(bytes: &[u8]) -> Result<Document, Fault> {
    if bytes.len() > super::super::skills::MAX_BYTES {
        return Err(Fault::invalid("skill document exceeds its resource budget"));
    }
    let content =
        std::str::from_utf8(bytes).map_err(|_| Fault::invalid("SKILL.md must be UTF-8 text"))?;
    let skill = super::super::skills::parse_document(content)?;
    Ok(Document {
        skill,
        content: super::super::skills::body(content).into(),
    })
}
