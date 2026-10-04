//! Inventory, queued callbacks, admitted turns, and bound assistants own plugin resources.
use super::*;
use std::collections::BTreeSet;

pub(in crate::store) fn run(db: &mut Connection, host: &crate::plugins::Host) {
    if let Err(error) = metadata(db).and_then(|retained| host.collect(&retained)) {
        // Optional maintenance must not block startup or hide a failed cleanup.
        eprintln!("plugin resource cleanup failed: {error}");
    }
}

fn metadata(db: &mut Connection) -> Result<BTreeSet<String>, Fault> {
    let transaction = db.transaction().map_err(storage_error)?;
    let mut packages = BTreeSet::new();
    let mut settings = BTreeSet::new();
    {
        let mut retain = |reference: plugin::Reference| {
            packages.insert(reference.digest);
            if reference.settings_revision != 0 {
                settings.insert((reference.name, reference.settings_revision));
            }
        };
        for summary in list(&transaction)? {
            retain(summary.reference());
        }
        let mut query = transaction
            .prepare("SELECT reference FROM dispatch_jobs WHERE status IN ('queued','running')")
            .map_err(storage_error)?;
        for body in query
            .query_map([], |row| row.get::<_, Vec<u8>>(0))
            .map_err(storage_error)?
        {
            let reference: plugin::Reference =
                serde_json::from_slice(&body.map_err(storage_error)?).map_err(storage_error)?;
            retain(reference);
        }
        let mut query = transaction
            .prepare("SELECT plugins FROM turns")
            .map_err(storage_error)?;
        for body in query
            .query_map([], |row| row.get::<_, Vec<u8>>(0))
            .map_err(storage_error)?
        {
            for reference in
                serde_json::from_slice::<Vec<plugin::Reference>>(&body.map_err(storage_error)?)
                    .map_err(storage_error)?
            {
                retain(reference);
            }
        }
        let mut query = transaction.prepare("SELECT json_extract(config,'$.assistant.package') FROM session_revisions WHERE json_extract(config,'$.assistant.package') IS NOT NULL").map_err(storage_error)?;
        for body in query
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(storage_error)?
        {
            retain(serde_json::from_str(&body.map_err(storage_error)?).map_err(storage_error)?);
        }
    }
    let mut unused = Vec::new();
    {
        let mut query = transaction
            .prepare("SELECT plugin,revision,body FROM plugin_settings")
            .map_err(storage_error)?;
        let mut rows = query.query([]).map_err(storage_error)?;
        while let Some(row) = rows.next().map_err(storage_error)? {
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
            .map_err(storage_error)?;
    }
    let digests = {
        let mut query = transaction
            .prepare("SELECT digest FROM plugin_packages")
            .map_err(storage_error)?;
        query
            .query_map([], |row| row.get::<_, String>(0))
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
            .map_err(storage_error)?;
    }
    // Disk cleanup follows the commit. Interrupted deletion leaves an orphan for the next run.
    transaction.commit().map_err(storage_error)?;
    Ok(packages)
}

#[cfg(test)]
mod tests;
