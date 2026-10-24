//! Inventory, queued callbacks, admitted turns, and bound assistants own plugin resources.
use super::*;
use std::collections::BTreeSet;

pub(in crate::store) async fn run(db: &mut Connection, host: &crate::plugins::Host) {
    if let Err(error) = metadata(db)
        .await
        .and_then(|retained| host.collect(&retained))
    {
        // Optional maintenance must not block startup or hide a failed cleanup.
        eprintln!("plugin resource cleanup failed: {error}");
    }
}

async fn metadata(db: &mut Connection) -> Result<BTreeSet<String>, Fault> {
    let transaction = db.transaction().await.map_err(storage_error)?;
    let mut packages = BTreeSet::new();
    let mut settings = BTreeSet::new();
    {
        let mut retain = |reference: plugin::Reference| {
            packages.insert(reference.digest);
            if reference.settings_revision != 0 {
                settings.insert((reference.name, reference.settings_revision));
            }
        };
        for summary in list(&transaction).await? {
            retain(summary.reference());
        }
        let mut query = transaction
            .prepare("SELECT reference FROM dispatch_jobs WHERE status IN ('queued','running')")
            .await
            .map_err(storage_error)?;
        {
            let mut rows = query.query(()).await.map_err(storage_error)?;
            while let Some(row) = rows.next().await.map_err(storage_error)? {
                let body: turso::Result<_> = (|row: &turso::Row| row.get::<Vec<u8>>(0))(&row);

                let reference: Option<plugin::Reference> =
                    serde_json::from_slice(&body.map_err(storage_error)?).map_err(storage_error)?;
                if let Some(reference) = reference {
                    retain(reference);
                }
            }
        }
        let mut query = transaction
            .prepare("SELECT plugins FROM turns")
            .await
            .map_err(storage_error)?;
        {
            let mut rows = query.query(()).await.map_err(storage_error)?;
            while let Some(row) = rows.next().await.map_err(storage_error)? {
                let body: turso::Result<_> = (|row: &turso::Row| row.get::<Vec<u8>>(0))(&row);

                for reference in
                    serde_json::from_slice::<Vec<plugin::Reference>>(&body.map_err(storage_error)?)
                        .map_err(storage_error)?
                {
                    retain(reference);
                }
            }
        }
        let mut query = transaction.prepare(
            "SELECT json_extract(provider,'$.binding.package') FROM agent_runs WHERE json_type(provider,'$.binding.package')='object'
             UNION ALL SELECT json_extract(profile,'$.provider.binding.package') FROM session_revisions WHERE json_type(profile,'$.provider.binding.package')='object'
             UNION ALL SELECT json_extract(item.value,'$.binding.package') FROM session_revisions, json_each(roles,'$.providers') item WHERE json_type(item.value,'$.binding.package')='object'
             UNION ALL SELECT json_extract(item.value,'$.binding.package') FROM turns,json_each(roles,'$.providers') item WHERE json_type(item.value,'$.binding.package')='object'"
        ).await.map_err(storage_error)?;
        {
            let mut rows = query.query(()).await.map_err(storage_error)?;
            while let Some(row) = rows.next().await.map_err(storage_error)? {
                let body: turso::Result<_> = (|row: &turso::Row| row.get::<String>(0))(&row);

                retain(serde_json::from_str(&body.map_err(storage_error)?).map_err(storage_error)?);
            }
        }
        let mut query = transaction.prepare("SELECT json_extract(config,'$.assistant.package') FROM session_revisions WHERE json_extract(config,'$.assistant.package') IS NOT NULL").await.map_err(storage_error)?;
        {
            let mut rows = query.query(()).await.map_err(storage_error)?;
            while let Some(row) = rows.next().await.map_err(storage_error)? {
                let body: turso::Result<_> = (|row: &turso::Row| row.get::<String>(0))(&row);

                retain(serde_json::from_str(&body.map_err(storage_error)?).map_err(storage_error)?);
            }
        }
    }
    let mut unused = Vec::new();
    {
        let mut query = transaction
            .prepare("SELECT plugin,revision,body FROM plugin_settings")
            .await
            .map_err(storage_error)?;
        let mut rows = query.query(()).await.map_err(storage_error)?;
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let name: String = row.get(0).map_err(storage_error)?;
            let revision: i64 = row.get(1).map_err(storage_error)?;
            if settings.contains(&(name.clone(), revision as u64)) {
                let body: Vec<u8> = row.get(2).map_err(storage_error)?;
                let _: super::settings::Stored =
                    serde_json::from_slice(&body).map_err(storage_error)?;
            } else {
                unused.push((name, revision));
            }
        }
    }
    for (name, revision) in unused {
        transaction
            .execute(
                "DELETE FROM plugin_settings WHERE plugin=?1 AND revision=?2",
                params![name, revision],
            )
            .await
            .map_err(storage_error)?;
    }
    let digests = {
        let mut query = transaction
            .prepare("SELECT digest FROM plugin_packages")
            .await
            .map_err(storage_error)?;
        (async {
            let mut rows = query.query(()).await?;
            let mut values = Vec::new();
            while let Some(row) = rows.next().await? {
                let mapped: turso::Result<_> = (|row: &turso::Row| row.get::<String>(0))(&row);
                values.push(mapped);
            }
            Ok::<_, turso::Error>(values.into_iter())
        })
        .await
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?
    };
    for digest in digests
        .into_iter()
        .filter(|digest| !packages.contains(digest))
    {
        transaction
            .execute("DELETE FROM plugin_packages WHERE digest=?1", [digest])
            .await
            .map_err(storage_error)?;
    }
    // Disk cleanup follows the commit. Interrupted deletion leaves an orphan for the next run.
    transaction.commit().await.map_err(storage_error)?;
    Ok(packages)
}

#[cfg(test)]
mod tests;
