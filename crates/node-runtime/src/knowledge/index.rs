//! Rebuildable retrieval data. Source registrations and grants stay Node-owned.
use super::{rank, text, vector::Vector};
use crate::store::database::{Handle, Lease};
use sailry_protocol::{ErrorCode, Fault, KnowledgeId, knowledge::*};
use std::{cmp::Ordering, collections::BinaryHeap, path::Path, sync::Arc};
use turso::{Connection, params, transaction::TransactionBehavior};

mod citations;
mod files;
mod query;
pub(super) use citations::Record;
#[cfg(test)]
mod tests;

const CANDIDATES: usize = 100;

#[derive(Clone)]
pub(super) struct Index {
    db: Arc<Handle>,
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

#[derive(Clone)]
pub(super) struct Binding {
    pub id: KnowledgeId,
    pub generation: u64,
    pub model: Option<(String, usize)>,
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
        .map_err(|_| Fault::invalid("knowledge integer exceeds the database range"))
}

impl Index {
    pub(super) async fn open(path: &Path) -> Result<Self, Fault> {
        let db = Arc::new(Handle::new(
            path.to_owned(),
            crate::store::database::engine(path).await.map_err(error)?,
        ));
        let index = Self { db };
        let mut lease = index.lease(false).await?;
        let connection = lease.connection_mut();
        // Schema creation uses the engine's ordinary DDL transaction. MVCC
        // readers remain independent; normal data writes use Concurrent.
        let tx = connection.transaction().await.map_err(error)?;
        tx.execute_batch(include_str!("index/schema.sql"))
            .await
            .map_err(error)?;
        let version: i64 = tx
            .prepare("SELECT version FROM metadata WHERE singleton=1")
            .await
            .map_err(error)?
            .query_row(())
            .await
            .map_err(error)?
            .get(0)
            .map_err(error)?;
        if version != 1 {
            return Err(error("unexpected derived index format"));
        }
        tx.commit().await.map_err(error)?;
        Ok(index)
    }

    pub(super) fn database(&self) -> Arc<Handle> {
        self.db.clone()
    }

    async fn lease(&self, readonly: bool) -> Result<Lease, Fault> {
        self.db.lease(readonly).await.map_err(error)
    }

    pub(super) async fn bind(&self, binding: &Binding) -> Result<(), Fault> {
        let generation = integer(binding.generation)?;
        let rebuild = integer(binding.rebuild)?;
        let dimension = binding
            .model
            .as_ref()
            .map(|(_, dimension)| integer(*dimension))
            .transpose()?;
        let mut lease = self.lease(false).await?;
        let connection = lease.connection_mut();
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Concurrent)
            .await
            .map_err(error)?;
        let model = binding.model.as_ref().map(|(model, _)| model.as_str());
        let previous = match tx
            .prepare("SELECT model,dimension,scope,rebuild FROM collections WHERE id=?1")
            .await
            .map_err(error)?
            .query_row([binding.id.to_string()])
            .await
        {
            Ok(row) => Some((
                row.get::<Option<String>>(0).map_err(error)?,
                row.get::<Option<i64>>(1).map_err(error)?,
                row.get::<Vec<u8>>(2).map_err(error)?,
                row.get::<i64>(3).map_err(error)?,
            )),
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(failure) => return Err(error(failure)),
        };
        if previous.as_ref().is_some_and(|previous| {
            previous.1.is_some_and(|dimension| dimension < 0) || previous.3 < 0
        }) {
            return Err(error("stored dimension or rebuild value is negative"));
        }
        tx.execute("INSERT INTO collections(id,generation,model,dimension,scope,rebuild) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET generation=excluded.generation,model=excluded.model,dimension=excluded.dimension,scope=excluded.scope,rebuild=excluded.rebuild",
            params![binding.id.to_string(), generation, model, dimension, binding.scope.as_slice(), rebuild]).await.map_err(error)?;
        if previous
            .as_ref()
            .is_some_and(|previous| previous.2 != binding.scope || previous.3 != rebuild)
        {
            tx.execute(
                "DELETE FROM sources WHERE collection=?1",
                [binding.id.to_string()],
            )
            .await
            .map_err(error)?;
        } else if previous.is_some_and(|previous| {
            (previous.0, previous.1) != (model.map(str::to_owned), dimension)
        }) {
            tx.execute("UPDATE chunks SET vector=NULL WHERE source IN (SELECT id FROM sources WHERE collection=?1)", [binding.id.to_string()]).await.map_err(error)?;
            tx.execute(
                "UPDATE sources SET embedded=0 WHERE collection=?1",
                [binding.id.to_string()],
            )
            .await
            .map_err(error)?;
        }
        tx.commit().await.map_err(error)
    }

    pub(super) async fn unchanged(
        &self,
        binding: &Binding,
        document: &Document,
    ) -> Result<bool, Fault> {
        let mut lease = self.lease(true).await?;
        let connection = lease.connection_mut();
        let tx = connection.transaction().await.map_err(error)?;
        if !current(&tx, binding).await? {
            return Ok(false);
        }
        let found = tx.prepare("SELECT EXISTS(SELECT 1 FROM sources s WHERE s.collection=?1 AND s.locator=?2 AND s.revision=?3 AND s.modified_ms=?4 AND s.tags=?5 AND (?6=0 OR s.embedded=1))").await.map_err(error)?
            .query_row(params![binding.id.to_string(), document.locator.as_str(), document.revision.as_str(), integer(document.modified_ms)?, serde_json::to_string(&document.tags).map_err(error)?, document.embedded]).await.map_err(error)?.get(0).map_err(error)?;
        tx.commit().await.map_err(error)?;
        Ok(found)
    }

    /// Publish one verified source atomically while preserving its public hash keys.
    pub(super) async fn replace(
        &self,
        binding: &Binding,
        document: &Document,
        parts: &[Part],
    ) -> Result<(), Fault> {
        let mut lease = self.lease(false).await?;
        let connection = lease.connection_mut();
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Concurrent)
            .await
            .map_err(error)?;
        if !current(&tx, binding).await? {
            return Err(Fault::new(
                ErrorCode::Cancelled,
                "knowledge collection changed",
            ));
        }
        let source: i64 = tx.prepare("INSERT INTO sources(collection,locator,revision,modified_ms,tags,embedded) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(collection,locator) DO UPDATE SET revision=excluded.revision,modified_ms=excluded.modified_ms,tags=excluded.tags,embedded=excluded.embedded RETURNING id").await.map_err(error)?
            .query_row(params![binding.id.to_string(), document.locator.as_str(), document.revision.as_str(), integer(document.modified_ms)?, serde_json::to_string(&document.tags).map_err(error)?, document.embedded]).await.map_err(error)?.get(0).map_err(error)?;
        tx.execute("DELETE FROM file_stamps WHERE source=?1", [source])
            .await
            .map_err(error)?;
        tx.execute("DELETE FROM chunks WHERE source=?1", [source])
            .await
            .map_err(error)?;
        for (ordinal, part) in parts.iter().enumerate() {
            if let Some(vector) = &part.vector {
                let (_, dimension) = binding
                    .model
                    .as_ref()
                    .ok_or_else(|| Fault::invalid("knowledge vector requires a captured model"))?;
                Vector::decode(&vector.encode(), *dimension)?;
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
            tx.execute("INSERT INTO chunks(key,source,ordinal,title,text,field0,field1,cjk0,cjk1,position,vector) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                params![key, source, integer(ordinal)?, part.title.as_str(), part.text.as_str(), text::field(&part.title), text::field(&part.text), text::index(&part.title), text::index(&part.text), serde_json::to_string(&part.position).map_err(error)?, part.vector.as_ref().map(Vector::encode)]).await.map_err(error)?;
        }
        tx.commit().await.map_err(error)
    }

    pub(super) async fn prune(&self, binding: &Binding, locators: &[String]) -> Result<(), Fault> {
        let mut lease = self.lease(false).await?;
        let connection = lease.connection_mut();
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Concurrent)
            .await
            .map_err(error)?;
        if !current(&tx, binding).await? {
            return Err(Fault::new(
                ErrorCode::Cancelled,
                "knowledge collection changed",
            ));
        }
        tx.execute("DELETE FROM sources WHERE collection=?1 AND locator NOT IN (SELECT value FROM json_each(?2))", params![binding.id.to_string(), serde_json::to_string(locators).map_err(error)?]).await.map_err(error)?;
        tx.commit().await.map_err(error)
    }

    pub(super) async fn retain(&self, ids: &[KnowledgeId]) -> Result<(), Fault> {
        let mut lease = self.lease(false).await?;
        let connection = lease.connection_mut();
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Concurrent)
            .await
            .map_err(error)?;
        tx.execute(
            "DELETE FROM collections WHERE id NOT IN (SELECT value FROM json_each(?1))",
            [serde_json::to_string(ids).map_err(error)?],
        )
        .await
        .map_err(error)?;
        tx.commit().await.map_err(error)
    }

    pub(super) async fn counts(&self, binding: &Binding) -> Result<(u64, u64), Fault> {
        let mut lease = self.lease(true).await?;
        let connection = lease.connection_mut();
        let tx = connection.transaction().await.map_err(error)?;
        if !current(&tx, binding).await? {
            return Ok((0, 0));
        }
        let row = tx.prepare("SELECT (SELECT count(*) FROM sources WHERE collection=?1),(SELECT count(*) FROM chunks c JOIN sources s ON s.id=c.source WHERE s.collection=?1)").await.map_err(error)?.query_row([binding.id.to_string()]).await.map_err(error)?;
        let sources = row.get::<i64>(0).map_err(error)?;
        let chunks = row.get::<i64>(1).map_err(error)?;
        tx.commit().await.map_err(error)?;
        Ok((
            sources.try_into().map_err(error)?,
            chunks.try_into().map_err(error)?,
        ))
    }
}

async fn current(db: &Connection, binding: &Binding) -> Result<bool, Fault> {
    db.prepare("SELECT EXISTS(SELECT 1 FROM collections WHERE id=?1 AND generation=?2 AND model IS ?3 AND dimension IS ?4 AND scope=?5 AND rebuild=?6)").await.map_err(error)?
        .query_row(params![binding.id.to_string(), integer(binding.generation)?, binding.model.as_ref().map(|(model, _)| model.as_str()), binding.model.as_ref().map(|(_, dimension)| integer(*dimension)).transpose()?, binding.scope.as_slice(), integer(binding.rebuild)?]).await.map_err(error)?
        .get(0).map_err(error)
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
