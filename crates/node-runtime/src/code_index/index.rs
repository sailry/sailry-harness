//! Derived data uses the existing Turso driver and its native transactional FTS.
use super::{check, parser::Parsed};
use crate::{
    knowledge::text,
    store::database::{Handle, Lease},
};
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, code_index::*};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};
use turso::{params, transaction::TransactionBehavior};

pub(super) const BATCH_SYMBOLS: usize = 512;

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(super) struct Index {
    handle: Arc<Handle>,
    #[cfg(test)]
    prepared: Arc<std::sync::Mutex<Option<Pause>>>,
}

#[cfg(test)]
type Pause = (
    tokio::sync::oneshot::Sender<()>,
    tokio::sync::oneshot::Receiver<()>,
);

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct State {
    pub language: Option<String>,
    pub status: Status,
    pub capabilities: Capabilities,
    pub partial: bool,
    pub truncated: bool,
}

impl From<&Parsed> for State {
    fn from(file: &Parsed) -> Self {
        Self {
            language: file.language.clone(),
            status: file.status,
            capabilities: file.capabilities.clone(),
            partial: file.partial,
            truncated: file.truncated,
        }
    }
}

pub(super) struct Cached {
    pub hash: Option<String>,
    pub fingerprint: String,
    pub state: State,
}

impl Index {
    pub(super) async fn open(path: &Path) -> Result<Self, Fault> {
        let index = Self {
            handle: Arc::new(Handle::new(
                path.to_owned(),
                crate::store::database::engine(path).await.map_err(error)?,
            )),
            #[cfg(test)]
            prepared: Default::default(),
        };
        let mut lease = index.lease(false).await?;
        let tx = lease.connection_mut().transaction().await.map_err(error)?;
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

    pub(super) async fn close(&self) -> Result<(), Fault> {
        match self.handle.pause().await {
            Ok(closed) => {
                drop(closed);
                Ok(())
            }
            Err(crate::Error::Stopped) => Ok(()),
            Err(cause) => Err(error(cause)),
        }
    }

    async fn lease(&self, readonly: bool) -> Result<Lease, Fault> {
        if readonly {
            self.handle.lease(true).await
        } else {
            self.handle.writer().await
        }
        .map_err(error)
    }

    #[cfg(test)]
    pub(super) fn hold_fixture(&self) -> Pause {
        let (entered, observed) = tokio::sync::oneshot::channel();
        let (release, resume) = tokio::sync::oneshot::channel();
        *self.prepared.lock().unwrap() = Some((entered, resume));
        (release, observed)
    }

    pub(super) async fn files(&self) -> Result<BTreeMap<String, Cached>, Fault> {
        let lease = self.lease(true).await?;
        let mut rows = lease
            .query("SELECT path,hash,fingerprint,state FROM files", ())
            .await
            .map_err(error)?;
        let mut files = BTreeMap::new();
        while let Some(row) = rows.next().await.map_err(error)? {
            let path = row.get(0).map_err(error)?;
            let state =
                serde_json::from_str(&row.get::<String>(3).map_err(error)?).map_err(error)?;
            files.insert(
                path,
                Cached {
                    hash: row.get(1).map_err(error)?,
                    fingerprint: row.get(2).map_err(error)?,
                    state,
                },
            );
        }
        Ok(files)
    }

    pub(super) async fn replace(
        &self,
        files: &[Parsed],
        fingerprint: &str,
        stop: &CancellationToken,
    ) -> Result<(), Fault> {
        if files.is_empty() {
            return Ok(());
        }
        check(stop)?;
        let mut lease = self.lease(false).await?;
        let tx = lease
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Concurrent)
            .await
            .map_err(error)?;
        for file in files {
            check(stop)?;
            let state = State::from(file);
            tx.execute("INSERT INTO files(path,fingerprint,hash,state) VALUES(?1,?2,?3,?4) ON CONFLICT(path) DO UPDATE SET fingerprint=excluded.fingerprint,hash=excluded.hash,state=excluded.state",
                params![file.path.as_str(),fingerprint,file.hash.as_deref(),serde_json::to_string(&state).map_err(error)?]).await.map_err(error)?;
            tx.execute("DELETE FROM symbols WHERE path=?1", [file.path.as_str()])
                .await
                .map_err(error)?;
        }
        // Turso 0.8.3 publishes native FTS at each statement boundary.
        // Multi-row inserts bound that work to 512 rows / 4096 parameters.
        let prefix = "INSERT INTO symbols(path,name,language,body,field0,field1,cjk0,cjk1) VALUES ";
        let mut sql = String::from(prefix);
        let mut values = Vec::with_capacity(BATCH_SYMBOLS * 8);
        for file in files {
            for symbol in &file.symbols {
                if !values.is_empty() {
                    sql.push(',');
                }
                sql.push_str("(?,?,?,?,?,?,?,?)");
                let context = format!(
                    "{} {}",
                    file.path,
                    symbol.signature.as_deref().unwrap_or_default()
                );
                values.extend([
                    file.path.clone(),
                    symbol.name.clone(),
                    symbol.language.clone(),
                    serde_json::to_string(symbol).map_err(error)?,
                    text::field(&symbol.name),
                    text::field(&context),
                    text::index(&symbol.name),
                    text::index(&context),
                ]);
                if values.len() == BATCH_SYMBOLS * 8 {
                    check(stop)?;
                    tx.execute(
                        &sql,
                        std::mem::replace(&mut values, Vec::with_capacity(BATCH_SYMBOLS * 8)),
                    )
                    .await
                    .map_err(error)?;
                    sql.truncate(prefix.len());
                }
            }
        }
        if !values.is_empty() {
            check(stop)?;
            tx.execute(&sql, values).await.map_err(error)?;
        }
        #[cfg(test)]
        {
            let pause = self.prepared.lock().unwrap().take();
            if let Some((entered, resume)) = pause {
                let _ = entered.send(());
                let _ = resume.await;
            }
        }
        tx.execute(
            "UPDATE metadata SET revision=revision+1 WHERE singleton=1",
            (),
        )
        .await
        .map_err(error)?;
        check(stop)?;
        tx.commit().await.map_err(error)
    }

    /// This is called only after a complete, successful directory traversal.
    pub(super) async fn prune(
        &self,
        seen: &BTreeSet<String>,
        stop: &CancellationToken,
    ) -> Result<(), Fault> {
        let paths: Vec<_> = self
            .files()
            .await?
            .into_keys()
            .filter(|path| !seen.contains(path))
            .collect();
        for paths in paths.chunks(64) {
            check(stop)?;
            let mut lease = self.lease(false).await?;
            let tx = lease
                .connection_mut()
                .transaction_with_behavior(TransactionBehavior::Concurrent)
                .await
                .map_err(error)?;
            for path in paths {
                // Delete explicitly so the native index sees every symbol removal.
                tx.execute("DELETE FROM symbols WHERE path=?1", [path.as_str()])
                    .await
                    .map_err(error)?;
                tx.execute("DELETE FROM files WHERE path=?1", [path.as_str()])
                    .await
                    .map_err(error)?;
            }
            tx.execute(
                "UPDATE metadata SET revision=revision+1 WHERE singleton=1",
                (),
            )
            .await
            .map_err(error)?;
            check(stop)?;
            tx.commit().await.map_err(error)?;
        }
        Ok(())
    }

    pub(super) async fn outline(&self, path: &str) -> Result<Outline, Fault> {
        let mut lease = self.lease(true).await?;
        let tx = lease.connection_mut().transaction().await.map_err(error)?;
        let row = tx
            .prepare("SELECT hash,state FROM files WHERE path=?1")
            .await
            .map_err(error)?
            .query_row([path])
            .await
            .map_err(error)?;
        let state: State =
            serde_json::from_str(&row.get::<String>(1).map_err(error)?).map_err(error)?;
        let mut rows = tx
            .query(
                "SELECT body FROM symbols WHERE path=?1 ORDER BY id LIMIT ?2",
                params![path, (MAX_OUTLINE_SYMBOLS + 1) as i64],
            )
            .await
            .map_err(error)?;
        let mut symbols = Vec::new();
        while let Some(row) = rows.next().await.map_err(error)? {
            symbols
                .push(serde_json::from_str(&row.get::<String>(0).map_err(error)?).map_err(error)?);
        }
        let truncated = state.truncated || symbols.len() > MAX_OUTLINE_SYMBOLS;
        symbols.truncate(MAX_OUTLINE_SYMBOLS);
        let outline = Outline {
            path: path.into(),
            language: state.language,
            file_revision: row.get(0).map_err(error)?,
            index_revision: revision(&tx).await?,
            status: state.status,
            symbols,
            capabilities: state.capabilities,
            partial: state.partial,
            truncated,
        };
        drop(rows);
        tx.commit().await.map_err(error)?;
        Ok(outline)
    }

    pub(super) async fn search(
        &self,
        query: &Search,
        stop: &CancellationToken,
    ) -> Result<SearchPage, Fault> {
        let mut lease = self.lease(true).await?;
        let tx = lease.connection_mut().transaction().await.map_err(error)?;
        let terms = text::query(&query.query);
        let filter = "(?1 IS NULL OR instr(lower(s.name),lower(?1))>0) AND (?2 IS NULL OR s.path=?2 OR substr(s.path,1,length(?2)+1)=?2||'/') AND (?3 IS NULL OR s.language=?3)";
        let limit = query.limit.min(MAX_SEARCH_SYMBOLS);
        let expression = ["field0", "field1", "cjk0", "cjk1"]
            .into_iter()
            .map(|field| format!("{field}:({terms})"))
            .collect::<Vec<_>>()
            .join(" OR ");
        let sql = if terms.is_empty() {
            format!("SELECT s.body FROM symbols s WHERE {filter} ORDER BY s.path,s.id LIMIT ?4")
        } else {
            // Materializing scores preserves order with secondary sorting in Turso 0.8.3.
            format!(
                "WITH candidates AS MATERIALIZED (SELECT id,fts_score(field0,field1,cjk0,cjk1,?5) AS score FROM symbols WHERE fts_match(field0,field1,cjk0,cjk1,?5)) SELECT s.body FROM candidates f JOIN symbols s ON s.id=f.id WHERE {filter} ORDER BY f.score DESC,s.path,s.id LIMIT ?4"
            )
        };
        let mut values = vec![
            turso::Value::from(query.name.as_deref()),
            turso::Value::from(query.path.as_deref()),
            turso::Value::from(query.language.as_deref()),
            turso::Value::from((limit + 1) as i64),
        ];
        if !terms.is_empty() {
            values.push(expression.into());
        }
        let mut rows = tx.query(&sql, values).await.map_err(error)?;
        let mut symbols = Vec::new();
        while let Some(row) = rows.next().await.map_err(error)? {
            check(stop)?;
            symbols
                .push(serde_json::from_str(&row.get::<String>(0).map_err(error)?).map_err(error)?);
        }
        let truncated = symbols.len() > limit;
        symbols.truncate(limit);
        let page = SearchPage {
            symbols,
            index_revision: revision(&tx).await?,
            partial: false,
            truncated,
            visited: 0,
            skipped: 0,
            issues: Vec::new(),
        };
        drop(rows);
        tx.commit().await.map_err(error)?;
        Ok(page)
    }
}

async fn revision(db: &turso::Connection) -> Result<u64, Fault> {
    let value: i64 = db
        .prepare("SELECT revision FROM metadata WHERE singleton=1")
        .await
        .map_err(error)?
        .query_row(())
        .await
        .map_err(error)?
        .get(0)
        .map_err(error)?;
    value.try_into().map_err(error)
}

fn error(error: impl std::fmt::Display) -> Fault {
    Fault::new(ErrorCode::Internal, format!("code index: {error}"))
}
