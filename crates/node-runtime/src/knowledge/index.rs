//! Rebuildable retrieval data. Source registrations and grants live in the Node store.
use super::{rank, text, vector::Vector};
use rusqlite::{Connection, OptionalExtension, params};
use sailry_protocol::{ErrorCode, Fault, KnowledgeId, knowledge::*};
use std::{cmp::Ordering, collections::BinaryHeap, path::Path};

mod citations;
mod files;
mod query;
pub(super) use citations::Record;
#[cfg(test)]
mod tests;

const CANDIDATES: usize = 100;

pub(super) struct Index {
    db: Connection,
}

pub(super) struct Document {
    pub locator: String,
    pub revision: String,
    pub modified_ms: u64,
    pub tags: Vec<String>,
    pub embedded: bool,
}

pub(super) struct Part {
    pub title: String,
    pub text: String,
    pub position: Position,
    pub vector: Option<Vector>,
}

#[derive(Clone, Copy)]
pub(super) struct Binding {
    pub id: KnowledgeId,
    pub generation: u64,
    pub model: Option<(Model, usize)>,
    pub scope: [u8; 32],
    pub rebuild: u64,
    pub paused: bool,
}

fn error(error: impl std::fmt::Display) -> Fault {
    Fault::new(ErrorCode::Internal, format!("knowledge index: {error}"))
}

fn integer(value: impl TryInto<i64>) -> Result<i64, Fault> {
    value
        .try_into()
        .map_err(|_| Fault::invalid("knowledge integer exceeds the SQLite range"))
}

impl Index {
    pub(super) fn open(path: &Path) -> Result<Self, Fault> {
        let db = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                | rusqlite::OpenFlags::SQLITE_OPEN_CREATE
                | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(error)?;
        db.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(error)?;
        db.execute_batch(include_str!("index/schema.sql"))
            .map_err(error)?;
        let extractor: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('file_stamps') WHERE name='extractor')", [], |row| row.get(0)).map_err(error)?;
        if !extractor {
            db.execute_batch("ALTER TABLE file_stamps ADD COLUMN extractor BLOB NOT NULL DEFAULT X'0000000000000000000000000000000000000000000000000000000000000000'").map_err(error)?;
        }
        let version: i64 = db
            .query_row(
                "SELECT version FROM metadata WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .map_err(error)?;
        if version != 1 {
            return Err(error("unexpected derived index format"));
        }
        Ok(Self { db })
    }

    pub(super) fn reader(path: &Path) -> Result<Self, Fault> {
        let db = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX
                | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(error)?;
        db.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(error)?;
        Ok(Self { db })
    }

    pub(super) fn bind(&mut self, binding: Binding) -> Result<(), Fault> {
        let generation = integer(binding.generation)?;
        let rebuild = integer(binding.rebuild)?;
        let dimension = binding
            .model
            .map(|(_, dimension)| integer(dimension))
            .transpose()?;
        let tx = self.db.transaction().map_err(error)?;
        let model = binding.model.map(|(model, _)| model.key());
        let previous = tx
            .query_row(
                "SELECT model,dimension,scope,rebuild FROM collections WHERE id=?1",
                [binding.id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(error)?;
        if previous.as_ref().is_some_and(|previous| {
            previous.1.is_some_and(|dimension| dimension < 0) || previous.3 < 0
        }) {
            return Err(error("stored dimension or rebuild value is negative"));
        }
        tx.execute(
            "INSERT INTO collections(id,generation,model,dimension,scope,rebuild) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET generation=excluded.generation,model=excluded.model,dimension=excluded.dimension,scope=excluded.scope,rebuild=excluded.rebuild",
            params![binding.id.to_string(), generation, model, dimension, binding.scope.as_slice(), rebuild],
        ).map_err(error)?;
        if previous
            .as_ref()
            .is_some_and(|previous| previous.2 != binding.scope || previous.3 != rebuild)
        {
            tx.execute(
                "DELETE FROM sources WHERE collection=?1",
                [binding.id.to_string()],
            )
            .map_err(error)?;
        } else if previous.is_some_and(|previous| {
            (previous.0, previous.1) != (model.map(std::borrow::Cow::into_owned), dimension)
        }) {
            tx.execute(
                "UPDATE chunks SET vector=NULL WHERE source IN (SELECT id FROM sources WHERE collection=?1)",
                [binding.id.to_string()],
            ).map_err(error)?;
            tx.execute(
                "UPDATE sources SET embedded=0 WHERE collection=?1",
                [binding.id.to_string()],
            )
            .map_err(error)?;
        }
        tx.commit().map_err(error)
    }

    fn current(&self, binding: Binding) -> Result<bool, Fault> {
        self.db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM collections WHERE id=?1 AND generation=?2 AND model IS ?3 AND dimension IS ?4 AND scope=?5 AND rebuild=?6)",
                params![
                    binding.id.to_string(),
                    integer(binding.generation)?,
                    binding.model.map(|(model, _)| model.key()),
                    binding.model.map(|(_, dimension)| integer(dimension)).transpose()?,
                    binding.scope.as_slice(),
                    integer(binding.rebuild)?,
                ],
                |row| row.get(0),
            )
            .map_err(error)
    }

    pub(super) fn unchanged(&self, binding: Binding, document: &Document) -> Result<bool, Fault> {
        if !self.current(binding)? {
            return Ok(false);
        }
        self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sources s WHERE s.collection=?1 AND s.locator=?2 AND s.revision=?3 AND s.modified_ms=?4 AND s.tags=?5 AND (?6=0 OR s.embedded=1))",
            params![binding.id.to_string(), document.locator, document.revision, integer(document.modified_ms)?, serde_json::to_string(&document.tags).map_err(error)?, document.embedded],
            |row| row.get(0),
        ).map_err(error)
    }

    /// Publish one verified source atomically; stale workers cannot restore deleted collections.
    pub(super) fn replace(
        &mut self,
        binding: Binding,
        document: &Document,
        parts: &[Part],
    ) -> Result<(), Fault> {
        if !self.current(binding)? {
            return Err(Fault::new(
                ErrorCode::Cancelled,
                "knowledge collection changed",
            ));
        }
        let tx = self.db.transaction().map_err(error)?;
        tx.execute(
            "INSERT INTO sources(collection,locator,revision,modified_ms,tags,embedded) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(collection,locator) DO UPDATE SET revision=excluded.revision,modified_ms=excluded.modified_ms,tags=excluded.tags,embedded=excluded.embedded",
            params![binding.id.to_string(), document.locator, document.revision, integer(document.modified_ms)?, serde_json::to_string(&document.tags).map_err(error)?, document.embedded],
        ).map_err(error)?;
        let source: i64 = tx
            .query_row(
                "SELECT id FROM sources WHERE collection=?1 AND locator=?2",
                params![binding.id.to_string(), document.locator],
                |row| row.get(0),
            )
            .map_err(error)?;
        tx.execute("DELETE FROM file_stamps WHERE source=?1", [source])
            .map_err(error)?;
        tx.execute("DELETE FROM chunks WHERE source=?1", [source])
            .map_err(error)?;
        for (ordinal, part) in parts.iter().enumerate() {
            if let Some(vector) = &part.vector {
                let (_, dimension) = binding
                    .model
                    .ok_or_else(|| Fault::invalid("knowledge vector requires a captured model"))?;
                Vector::decode(&vector.encode(), dimension)?;
            }
            let key = blake3::hash(
                &serde_json::to_vec(&(
                    binding.id,
                    &document.locator,
                    &document.revision,
                    ordinal,
                    &part.position,
                    blake3::hash(part.text.as_bytes()).as_bytes(),
                ))
                .map_err(error)?,
            )
            .to_hex()
            .to_string();
            tx.execute(
                "INSERT INTO chunks(key,source,ordinal,title,text,position,vector) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![key, source, integer(ordinal)?, part.title, part.text, serde_json::to_string(&part.position).map_err(error)?, part.vector.as_ref().map(Vector::encode)],
            ).map_err(error)?;
            tx.execute(
                "INSERT INTO chunks_fts(rowid,title,text,cjk_title,cjk_text) VALUES(?1,?2,?3,?4,?5)",
                params![tx.last_insert_rowid(), part.title, part.text, text::index(&part.title), text::index(&part.text)],
            ).map_err(error)?;
        }
        tx.commit().map_err(error)
    }

    pub(super) fn prune(&mut self, binding: Binding, locators: &[String]) -> Result<(), Fault> {
        if !self.current(binding)? {
            return Err(Fault::new(
                ErrorCode::Cancelled,
                "knowledge collection changed",
            ));
        }
        self.db.execute(
            "DELETE FROM sources WHERE collection=?1 AND locator NOT IN (SELECT value FROM json_each(?2))",
            params![binding.id.to_string(), serde_json::to_string(locators).map_err(error)?],
        ).map_err(error)?;
        Ok(())
    }

    pub(super) fn retain(&mut self, ids: &[KnowledgeId]) -> Result<(), Fault> {
        self.db
            .execute(
                "DELETE FROM collections WHERE id NOT IN (SELECT value FROM json_each(?1))",
                [serde_json::to_string(ids).map_err(error)?],
            )
            .map_err(error)?;
        Ok(())
    }

    pub(super) fn counts(&self, binding: Binding) -> Result<(u64, u64), Fault> {
        if !self.current(binding)? {
            return Ok((0, 0));
        }
        let (sources, chunks) = self.db.query_row(
            "SELECT (SELECT count(*) FROM sources WHERE collection=?1),(SELECT count(*) FROM chunks c JOIN sources s ON s.id=c.source WHERE s.collection=?1)",
            [binding.id.to_string()],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        ).map_err(error)?;
        Ok((
            u64::try_from(sources).map_err(error)?,
            u64::try_from(chunks).map_err(error)?,
        ))
    }
}

struct Scored {
    key: String,
    score: f32,
}

impl PartialEq for Scored {
    fn eq(&self, other: &Self) -> bool {
        self.score.total_cmp(&other.score).is_eq() && self.key == other.key
    }
}
impl Eq for Scored {}
impl PartialOrd for Scored {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Scored {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .total_cmp(&other.score)
            .then_with(|| other.key.cmp(&self.key))
    }
}

fn retain(heap: &mut BinaryHeap<std::cmp::Reverse<Scored>>, value: Scored) {
    if heap.len() < CANDIDATES {
        heap.push(std::cmp::Reverse(value));
    } else if heap.peek().is_some_and(|worst| value > worst.0) {
        heap.pop();
        heap.push(std::cmp::Reverse(value));
    }
}
