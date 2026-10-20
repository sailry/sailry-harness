//! Import the retired local model stores once, inside the startup transaction.
use crate::{Error, models::registry::catalog, store::local_models};
use rusqlite::{Connection, OptionalExtension, params};
use sailry_protocol::{
    ModelId,
    models::{Choice, Definition, Kind, Settings},
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Model {
    GraniteMulti,
    BgeSmallZh,
    PotionMulti,
    Registered { id: ModelId },
}

impl Model {
    fn key(self) -> String {
        match self {
            Self::GraniteMulti => "granite-multi".into(),
            Self::BgeSmallZh => "bge-small-zh".into(),
            Self::PotionMulti => "potion-multi".into(),
            Self::Registered { id } => format!("model-{id}"),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Embedding {
    revision: u64,
    model: Option<Model>,
    mirror: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ocr {
    settings: OcrSettings,
    records: Vec<Definition>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OcrSettings {
    revision: u64,
    enabled: bool,
    model: Option<String>,
}

pub(super) fn reconcile(db: &Connection) -> Result<bool, Error> {
    let imported: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM local_model_settings WHERE singleton=1)",
        [],
        |row| row.get(0),
    )?;
    // The singleton records completed import even when every registration is removed.
    if imported {
        return Ok(false);
    }
    let mut settings = Settings::default();
    if let Some(body) = singleton(db, "knowledge_settings")? {
        let previous: Embedding = decode(&body)?;
        if previous.revision > i64::MAX as u64 {
            return Err(invalid());
        }
        settings.mirror = previous.mirror;
        settings.choices.insert(
            Kind::Embedding,
            Choice {
                enabled: previous.model.is_some(),
                model: previous.model.map(Model::key),
            },
        );
    }
    if exists(db, "knowledge_models")? {
        let mut query = db.prepare("SELECT id,digest,body FROM knowledge_models ORDER BY id")?;
        let records = query.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })?;
        for record in records {
            let (id, digest, body) = record?;
            let id: ModelId = id.parse().map_err(|_| invalid())?;
            if blake3::hash(&body).to_hex().as_str() != digest {
                return Err(invalid());
            }
            let definition: Definition = decode(&body)?;
            if definition.format.kind() != Kind::Embedding {
                return Err(invalid());
            }
            insert(db, &format!("model-{id}"), &definition)?;
        }
    }
    if let Some(body) = singleton(db, "ocr_settings")? {
        let previous: Ocr = decode(&body)?;
        if previous.settings.revision > i64::MAX as u64 {
            return Err(invalid());
        }
        settings.choices.insert(
            Kind::Ocr,
            Choice {
                enabled: previous.settings.enabled,
                model: previous.settings.model,
            },
        );
        for definition in previous.records {
            if definition.format.kind() != Kind::Ocr {
                return Err(invalid());
            }
            insert(db, &catalog::id(&definition), &definition)?;
        }
    }
    settings.validate().map_err(|_| invalid())?;
    db.execute(
        "INSERT INTO local_model_settings(singleton,body) VALUES(1,?1)",
        [encode(&settings)?],
    )?;
    local_models::read(db).map_err(|_| invalid())?;
    collections(db)?;
    captures(db)?;
    Ok(true)
}

fn insert(db: &Connection, id: &str, definition: &Definition) -> Result<(), Error> {
    definition.validate().map_err(|_| invalid())?;
    let previous: Option<Vec<u8>> = db
        .query_row("SELECT body FROM local_models WHERE id=?1", [id], |row| {
            row.get(0)
        })
        .optional()?;
    if let Some(previous) = previous {
        return if decode::<Definition>(&previous)? == *definition {
            Ok(())
        } else {
            Err(invalid())
        };
    }
    db.execute(
        "INSERT INTO local_models(id,kind,digest,body) VALUES(?1,?2,?3,?4)",
        params![
            id,
            local_models::kind_name(definition.format.kind()),
            catalog::id(definition),
            encode(definition)?
        ],
    )?;
    Ok(())
}

fn exists(db: &Connection, table: &str) -> Result<bool, Error> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [table],
        |row| row.get(0),
    )?)
}

fn singleton(db: &Connection, table: &str) -> Result<Option<Vec<u8>>, Error> {
    if !exists(db, table)? {
        return Ok(None);
    }
    Ok(db
        .query_row(
            &format!("SELECT body FROM {table} WHERE singleton=1"),
            [],
            |row| row.get(0),
        )
        .optional()?)
}

fn collections(db: &Connection) -> Result<(), Error> {
    transform(db, "knowledge_collections", "body", |value| {
        let collection = value.get_mut("collection").ok_or_else(invalid)?;
        collection_refs(collection)
    })
}

fn reference(value: &mut Value) -> Result<bool, Error> {
    if value.is_null() {
        return Ok(false);
    }
    let model: Model = serde_json::from_value(value.clone()).map_err(|_| invalid())?;
    let current = Value::String(model.key());
    let changed = *value != current;
    *value = current;
    Ok(changed)
}

fn collection_refs(value: &mut Value) -> Result<bool, Error> {
    let mut changed = false;
    if let Some(model) = value
        .get_mut("definition")
        .and_then(|value| value.get_mut("model"))
    {
        changed |= reference(model)?;
    }
    if let Some(model) = value
        .get_mut("progress")
        .and_then(|value| value.get_mut("model"))
    {
        changed |= reference(model)?;
    }
    Ok(changed)
}

fn captures(db: &Connection) -> Result<(), Error> {
    transform(db, "agent_approvals", "prepared", |value| {
        let Some(command) = value.get_mut("command") else {
            return Ok(false);
        };
        let kind = command.get("kind").and_then(Value::as_str);
        let current = match kind {
            Some("install_ocr_model") => "install_local_model",
            Some("delete_ocr_model") => "delete_local_model",
            _ => return Ok(false),
        };
        command["kind"] = Value::String(current.into());
        let command: sailry_protocol::Command =
            serde_json::from_value(command.clone()).map_err(|_| invalid())?;
        value["digest"] = Value::String(blake3::hash(&encode(&command)?).to_hex().to_string());
        Ok(true)
    })?;
    // Only typed public snapshots are adapted. User and tool payloads remain opaque.
    transform(db, "requests", "result", |value| {
        let Some(output) = value.get_mut("Ok") else {
            return Ok(false);
        };
        match output.get("kind").and_then(Value::as_str) {
            Some("knowledge_collection") => {
                output.get_mut("data").map_or(Ok(false), collection_refs)
            }
            Some("knowledge_collections") => {
                let Some(entries) = output.get_mut("data").and_then(Value::as_array_mut) else {
                    return Err(invalid());
                };
                let mut changed = false;
                for entry in entries {
                    changed |= collection_refs(entry)?;
                }
                Ok(changed)
            }
            Some("knowledge_search") => output
                .get_mut("data")
                .and_then(|value| value.get_mut("model"))
                .map_or(Ok(false), reference),
            _ => Ok(false),
        }
    })
}

fn transform(
    db: &Connection,
    table: &str,
    column: &str,
    mut update: impl FnMut(&mut Value) -> Result<bool, Error>,
) -> Result<(), Error> {
    if !exists(db, table)? {
        return Ok(());
    }
    let select = format!(
        "SELECT rowid,CAST({column} AS BLOB) FROM {table} WHERE rowid>=?1 AND {column} IS NOT NULL ORDER BY rowid LIMIT 1"
    );
    let mut query = db.prepare(&select)?;
    let mut next = Some(i64::MIN);
    while let Some(after) = next {
        let record = query
            .query_row([after], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .optional()?;
        let Some((row, before)) = record else { break };
        next = row.checked_add(1);
        let mut value: Value = decode(&before)?;
        if update(&mut value)? {
            let sql = format!("UPDATE {table} SET {column}=?1 WHERE rowid=?2");
            if table == "requests" {
                db.execute(
                    &sql,
                    params![serde_json::to_string(&value).map_err(|_| invalid())?, row],
                )?;
            } else {
                db.execute(&sql, params![encode(&value)?, row])?;
            }
        }
    }
    Ok(())
}

fn decode<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, Error> {
    serde_json::from_slice(body).map_err(|_| invalid())
}

fn encode(value: &impl serde::Serialize) -> Result<Vec<u8>, Error> {
    serde_json::to_vec(value).map_err(|_| invalid())
}

fn invalid() -> Error {
    Error::Worker("stored local models cannot be imported; existing data was not changed".into())
}
#[cfg(test)]
mod tests;
