//! Reference and import rows commit with their file's symbols. Graph reads
//! share one transaction, so a query observes a single index revision.
use super::super::{
    graph::{Candidate, definition},
    parser::{Scope, Usage},
};
use super::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::atomic::{AtomicBool, Ordering},
};

const BATCH_ROWS: usize = 256;
/// Parameters in one `IN` list.
const CHUNK: usize = 256;
/// Definitions kept per name; more mark the query partial.
const MAX_CANDIDATES: usize = 1024;
/// Matches the parser's per-file declaration limit.
const MAX_OUTLINE: i64 = 4096;
const COLUMNS: &str = "name,kind,start_line,start_column,start_byte,receiver,scope,scope_kind,scope_line,scope_end_line";

pub(super) async fn forget(db: &turso::Connection, path: &str) -> Result<(), Fault> {
    db.execute("DELETE FROM refs WHERE path=?1", [path])
        .await
        .map_err(error)?;
    db.execute("DELETE FROM imports WHERE path=?1", [path])
        .await
        .map_err(error)?;
    Ok(())
}

pub(super) async fn record(
    db: &turso::Connection,
    files: &[Parsed],
    stop: &CancellationToken,
) -> Result<(), Fault> {
    let mut batch = Batch::new(
        "INSERT INTO refs(path,name,kind,start_line,start_column,start_byte,receiver,scope,scope_kind,scope_line,scope_end_line) VALUES ",
        "(?,?,?,?,?,?,?,?,?,?,?)",
    );
    for file in files {
        for usage in &file.references {
            let scope = usage.scope.as_ref();
            batch
                .push(
                    db,
                    [
                        turso::Value::from(file.path.as_str()),
                        turso::Value::from(usage.name.as_str()),
                        turso::Value::from(usage.kind.as_str()),
                        turso::Value::from(usage.line),
                        turso::Value::from(usage.column),
                        turso::Value::from(usage.start),
                        turso::Value::from(i64::from(usage.receiver)),
                        turso::Value::from(scope.map(|scope| scope.name.as_str())),
                        turso::Value::from(scope.map(|scope| scope.kind.as_str())),
                        turso::Value::from(scope.map(|scope| scope.start_line)),
                        turso::Value::from(scope.map(|scope| scope.end_line)),
                    ],
                    stop,
                )
                .await?;
        }
    }
    batch.flush(db, stop).await?;
    let mut batch = Batch::new("INSERT INTO imports(path,module) VALUES ", "(?,?)");
    for file in files {
        for module in &file.imports {
            batch
                .push(
                    db,
                    [
                        turso::Value::from(file.path.as_str()),
                        turso::Value::from(module.as_str()),
                    ],
                    stop,
                )
                .await?;
        }
    }
    batch.flush(db, stop).await
}

/// Multi-row inserts bounded like symbol batches.
struct Batch {
    prefix: &'static str,
    row: &'static str,
    sql: String,
    values: Vec<turso::Value>,
    rows: usize,
}

impl Batch {
    fn new(prefix: &'static str, row: &'static str) -> Self {
        Self {
            prefix,
            row,
            sql: String::from(prefix),
            values: Vec::new(),
            rows: 0,
        }
    }

    async fn push(
        &mut self,
        db: &turso::Connection,
        values: impl IntoIterator<Item = turso::Value>,
        stop: &CancellationToken,
    ) -> Result<(), Fault> {
        if self.rows > 0 {
            self.sql.push(',');
        }
        self.sql.push_str(self.row);
        self.values.extend(values);
        self.rows += 1;
        if self.rows == BATCH_ROWS {
            self.flush(db, stop).await?;
        }
        Ok(())
    }

    async fn flush(
        &mut self,
        db: &turso::Connection,
        stop: &CancellationToken,
    ) -> Result<(), Fault> {
        if self.rows == 0 {
            return Ok(());
        }
        check(stop)?;
        db.execute(&self.sql, std::mem::take(&mut self.values))
            .await
            .map_err(error)?;
        self.sql.truncate(self.prefix.len());
        self.rows = 0;
        Ok(())
    }
}

pub(in super::super) struct Reader<'a> {
    db: &'a turso::Connection,
    stop: &'a CancellationToken,
    limited: AtomicBool,
}

impl<'a> Reader<'a> {
    pub(super) fn new(db: &'a turso::Connection, stop: &'a CancellationToken) -> Self {
        Self {
            db,
            stop,
            limited: AtomicBool::new(false),
        }
    }

    pub fn check(&self) -> Result<(), Fault> {
        check(self.stop)
    }

    /// Whether a bounded read omitted rows.
    pub fn limited(&self) -> bool {
        self.limited.load(Ordering::Relaxed)
    }

    pub fn limit(&self) {
        self.limited.store(true, Ordering::Relaxed);
    }

    /// Definitions with exactly these names, in path order.
    pub async fn definitions(
        &self,
        names: &BTreeSet<String>,
    ) -> Result<BTreeMap<String, Vec<Candidate>>, Fault> {
        let names: Vec<&String> = names.iter().collect();
        let mut found: BTreeMap<String, Vec<Candidate>> = BTreeMap::new();
        let mut seen = BTreeSet::new();
        for chunk in names.chunks(CHUNK) {
            self.check()?;
            let sql = format!(
                "SELECT body FROM symbols WHERE name IN ({}) ORDER BY path,id",
                placeholders(chunk.len())
            );
            let values: Vec<turso::Value> = chunk
                .iter()
                .map(|name| turso::Value::from(name.as_str()))
                .collect();
            let mut rows = self.db.query(&sql, values).await.map_err(error)?;
            while let Some(row) = rows.next().await.map_err(error)? {
                let candidate = candidate(&row)?;
                if !definition(&candidate.kind) {
                    continue;
                }
                let entry = found.entry(candidate.name.clone()).or_default();
                if entry.len() >= MAX_CANDIDATES {
                    self.limit();
                    continue;
                }
                if seen.insert((
                    candidate.path.clone(),
                    candidate.name.clone(),
                    candidate.start_line,
                )) {
                    entry.push(candidate);
                }
            }
        }
        Ok(found)
    }

    /// Every indexed declaration in one file, in source order.
    pub async fn outline(&self, path: &str) -> Result<Vec<Candidate>, Fault> {
        self.check()?;
        let mut rows = self
            .db
            .query(
                "SELECT body FROM symbols WHERE path=?1 ORDER BY id LIMIT ?2",
                params![path, MAX_OUTLINE],
            )
            .await
            .map_err(error)?;
        let mut found = Vec::new();
        while let Some(row) = rows.next().await.map_err(error)? {
            found.push(candidate(&row)?);
        }
        Ok(found)
    }

    /// Definitions in files equal to or below the given paths.
    pub async fn within(&self, paths: &[String], limit: usize) -> Result<Vec<Candidate>, Fault> {
        let mut found = Vec::new();
        let mut seen = BTreeSet::new();
        for path in paths {
            self.check()?;
            let mut rows = self
                .db
                .query(
                    "SELECT body FROM symbols WHERE path=?1 OR substr(path,1,length(?1)+1)=?1||'/' ORDER BY path,id",
                    [path.as_str()],
                )
                .await
                .map_err(error)?;
            while let Some(row) = rows.next().await.map_err(error)? {
                let candidate = candidate(&row)?;
                if !definition(&candidate.kind)
                    || !seen.insert((
                        candidate.path.clone(),
                        candidate.name.clone(),
                        candidate.start_line,
                    ))
                {
                    continue;
                }
                if found.len() == limit {
                    self.limit();
                    return Ok(found);
                }
                found.push(candidate);
            }
        }
        Ok(found)
    }

    /// References to any of the names, ordered by file and position.
    pub async fn references(
        &self,
        names: &BTreeSet<String>,
        limit: usize,
    ) -> Result<Vec<(String, Usage)>, Fault> {
        let names: Vec<&String> = names.iter().collect();
        let mut found = Vec::new();
        for chunk in names.chunks(CHUNK) {
            self.check()?;
            let remaining = limit.saturating_sub(found.len());
            if remaining == 0 {
                self.limit();
                break;
            }
            let sql = format!(
                "SELECT path,{COLUMNS} FROM refs WHERE name IN ({}) ORDER BY path,start_byte LIMIT ?",
                placeholders(chunk.len())
            );
            let mut values: Vec<turso::Value> = chunk
                .iter()
                .map(|name| turso::Value::from(name.as_str()))
                .collect();
            values.push(turso::Value::from(remaining as i64 + 1));
            let mut rows = self.db.query(&sql, values).await.map_err(error)?;
            while let Some(row) = rows.next().await.map_err(error)? {
                if found.len() == limit {
                    self.limit();
                    break;
                }
                found.push((row.get::<String>(0).map_err(error)?, usage(&row, 1)?));
            }
        }
        Ok(found)
    }

    /// References starting inside a byte range of one file.
    pub async fn inside(
        &self,
        path: &str,
        start: u32,
        end: u32,
        limit: usize,
    ) -> Result<Vec<Usage>, Fault> {
        self.check()?;
        let mut rows = self
            .db
            .query(
                &format!(
                    "SELECT {COLUMNS} FROM refs WHERE path=?1 AND start_byte>=?2 AND start_byte<?3 ORDER BY start_byte LIMIT ?4"
                ),
                params![path, start, end, limit as i64 + 1],
            )
            .await
            .map_err(error)?;
        let mut found = Vec::new();
        while let Some(row) = rows.next().await.map_err(error)? {
            if found.len() == limit {
                self.limit();
                break;
            }
            found.push(usage(&row, 0)?);
        }
        Ok(found)
    }

    /// Module keys imported by each of the files.
    pub async fn imports(
        &self,
        paths: &BTreeSet<String>,
    ) -> Result<BTreeMap<String, Vec<String>>, Fault> {
        let paths: Vec<&String> = paths.iter().collect();
        let mut found: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for chunk in paths.chunks(CHUNK) {
            self.check()?;
            let sql = format!(
                "SELECT path,module FROM imports WHERE path IN ({})",
                placeholders(chunk.len())
            );
            let values: Vec<turso::Value> = chunk
                .iter()
                .map(|path| turso::Value::from(path.as_str()))
                .collect();
            let mut rows = self.db.query(&sql, values).await.map_err(error)?;
            while let Some(row) = rows.next().await.map_err(error)? {
                found
                    .entry(row.get(0).map_err(error)?)
                    .or_default()
                    .push(row.get(1).map_err(error)?);
            }
        }
        Ok(found)
    }

    /// Imports of every file, bounded.
    pub async fn modules(&self, limit: usize) -> Result<BTreeMap<String, Vec<String>>, Fault> {
        self.check()?;
        let mut rows = self
            .db
            .query(
                "SELECT path,module FROM imports ORDER BY path LIMIT ?1",
                params![limit as i64 + 1],
            )
            .await
            .map_err(error)?;
        let mut found: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut count = 0;
        while let Some(row) = rows.next().await.map_err(error)? {
            if count == limit {
                self.limit();
                break;
            }
            count += 1;
            found
                .entry(row.get(0).map_err(error)?)
                .or_default()
                .push(row.get(1).map_err(error)?);
        }
        Ok(found)
    }

    /// Reference counts per referencing file and name, bounded.
    pub async fn totals(&self, limit: usize) -> Result<Vec<(String, String, u32)>, Fault> {
        self.check()?;
        let mut rows = self
            .db
            .query(
                "SELECT path,name,count(*) FROM refs GROUP BY path,name ORDER BY path,name LIMIT ?1",
                params![limit as i64 + 1],
            )
            .await
            .map_err(error)?;
        let mut found = Vec::new();
        while let Some(row) = rows.next().await.map_err(error)? {
            if found.len() == limit {
                self.limit();
                break;
            }
            let count: i64 = row.get(2).map_err(error)?;
            found.push((
                row.get(0).map_err(error)?,
                row.get(1).map_err(error)?,
                u32::try_from(count).unwrap_or(u32::MAX),
            ));
        }
        Ok(found)
    }

    /// Indexed declarations per file.
    pub async fn counts(&self) -> Result<BTreeMap<String, u32>, Fault> {
        self.check()?;
        let mut rows = self
            .db
            .query("SELECT path,count(*) FROM symbols GROUP BY path", ())
            .await
            .map_err(error)?;
        let mut found = BTreeMap::new();
        while let Some(row) = rows.next().await.map_err(error)? {
            let count: i64 = row.get(1).map_err(error)?;
            found.insert(
                row.get::<String>(0).map_err(error)?,
                u32::try_from(count).unwrap_or(u32::MAX),
            );
        }
        Ok(found)
    }
}

fn candidate(row: &turso::Row) -> Result<Candidate, Fault> {
    let symbol: Symbol =
        serde_json::from_str(&row.get::<String>(0).map_err(error)?).map_err(error)?;
    Ok(Candidate::from(symbol))
}

fn usage(row: &turso::Row, first: usize) -> Result<Usage, Fault> {
    let scope: Option<String> = row.get(first + 6).map_err(error)?;
    let scope = match scope {
        Some(name) => Some(Scope {
            name,
            kind: row
                .get::<Option<String>>(first + 7)
                .map_err(error)?
                .unwrap_or_default(),
            start_line: row
                .get::<Option<u32>>(first + 8)
                .map_err(error)?
                .unwrap_or_default(),
            end_line: row
                .get::<Option<u32>>(first + 9)
                .map_err(error)?
                .unwrap_or_default(),
        }),
        None => None,
    };
    Ok(Usage {
        name: row.get(first).map_err(error)?,
        kind: row.get(first + 1).map_err(error)?,
        line: row.get(first + 2).map_err(error)?,
        column: row.get(first + 3).map_err(error)?,
        start: row.get(first + 4).map_err(error)?,
        receiver: row.get::<i64>(first + 5).map_err(error)? != 0,
        scope,
    })
}

fn placeholders(count: usize) -> String {
    vec!["?"; count].join(",")
}

#[cfg(test)]
mod tests;
