use super::*;
use sailry_protocol::{
    ModelId,
    knowledge::Registration,
    models::{Definition, Kind},
};

pub(super) fn load(db: &Connection) -> Result<Vec<Registration>, Fault> {
    let mut statement = db
        .prepare("SELECT id,digest,body FROM knowledge_models ORDER BY id")
        .map_err(storage_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })
        .map_err(storage_error)?;
    let entries = rows
        .map(|row| {
            let (id, digest, body) = row.map_err(storage_error)?;
            if blake3::hash(&body).to_hex().as_str() != digest {
                return Err(storage_error(
                    "registered model digest does not match its definition",
                ));
            }
            let definition: Definition = serde_json::from_slice(&body).map_err(storage_error)?;
            definition.validate()?;
            if definition.format.kind() != Kind::Embedding {
                return Err(Fault::invalid(
                    "knowledge model requires an embedding format",
                ));
            }
            Ok(Registration {
                model: knowledge::Model::Registered {
                    id: id.parse().map_err(storage_error)?,
                },
                definition,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if entries.len() > 128 {
        return Err(storage_error("embedding model catalog capacity exceeded"));
    }
    Ok(entries)
}

pub(super) fn require(db: &Connection, model: Option<knowledge::Model>) -> Result<(), Fault> {
    if let Some(knowledge::Model::Registered { id }) = model {
        let present: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM knowledge_models WHERE id=?1)",
                [id.to_string()],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if !present {
            return Err(Fault::new(
                ErrorCode::NotFound,
                "embedding model is not registered",
            ));
        }
    }
    Ok(())
}

pub(super) fn register(
    db: &Connection,
    definition: &Definition,
) -> Result<(Output, Option<Event>), Fault> {
    definition.validate()?;
    if definition.format.kind() != Kind::Embedding {
        return Err(Fault::invalid(
            "knowledge model requires an embedding format",
        ));
    }
    let mut definition = definition.clone();
    definition
        .assets
        .sort_by(|left, right| left.name.cmp(&right.name));
    for asset in &mut definition.assets {
        asset.source.revision.make_ascii_lowercase();
        asset.sha256.make_ascii_lowercase();
    }
    let body = encode(&definition)?;
    let digest = blake3::hash(&body).to_hex().to_string();
    let previous: Option<String> = db
        .query_row(
            "SELECT id FROM knowledge_models WHERE digest=?1",
            [&digest],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    let id = if let Some(previous) = previous {
        previous.parse().map_err(storage_error)?
    } else {
        let count: i64 = db
            .query_row("SELECT count(*) FROM knowledge_models", [], |row| {
                row.get(0)
            })
            .map_err(storage_error)?;
        if count >= 128 {
            return Err(Fault::new(
                ErrorCode::Busy,
                "embedding model catalog is full",
            ));
        }
        let id = ModelId::new();
        db.execute(
            "INSERT INTO knowledge_models(id,digest,body) VALUES(?1,?2,?3)",
            params![id.to_string(), digest, body],
        )
        .map_err(storage_error)?;
        id
    };
    Ok((
        Output::KnowledgeModel {
            model: knowledge::Model::Registered { id },
        },
        None,
    ))
}
