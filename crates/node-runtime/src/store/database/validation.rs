//! Check typed stored JSON before startup changes journal or recovery state.
use crate::Error;
use sailry_protocol::{
    Event, Fault, Output, SessionConfig, SessionProfile, conversation::Provider, role,
};
use serde::de::DeserializeOwned;
use turso::{Connection, Value};

#[cfg(test)]
#[path = "validation/startup_tests.rs"]
mod startup_tests;

/// Complete validation remains mandatory for upgrades and backup acceptance.
pub(in crate::store) async fn stored(db: &Connection) -> Result<(), Error> {
    validate(db, true).await
}

/// Validate state that startup can consume or mutate; archives validate on read.
pub(super) async fn startup(db: &Connection) -> Result<(), Error> {
    validate(db, false).await
}

const LIVE_TURNS: &str = "SELECT turn FROM agent_pending
    UNION SELECT turn FROM agent_runs WHERE json_extract(body,'$.status')='queued'
    UNION SELECT turn FROM agent_runs WHERE json_extract(body,'$.status')='running'
    UNION SELECT turn FROM agent_runs WHERE json_extract(body,'$.status')='stopping'
    UNION SELECT turn FROM agent_approvals WHERE json_extract(body,'$.state')='pending'
    UNION SELECT turn FROM agent_questions WHERE json_extract(body,'$.state.kind')='pending'
    UNION SELECT turn FROM agent_background WHERE state='running'
    UNION SELECT turn FROM agent_background WHERE state='pending'
    UNION SELECT delivery FROM agent_background WHERE state='running' AND delivery IS NOT NULL
    UNION SELECT delivery FROM agent_background WHERE state='pending' AND delivery IS NOT NULL";

async fn validate(db: &Connection, archives: bool) -> Result<(), Error> {
    if db.prepare("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='power_settings')").await?.query_row(()).await?.get::<bool>(0)? {
        records::<sailry_protocol::power::Settings>(
            db,
            "power_settings.body",
            "SELECT body FROM power_settings",
        ).await?;
    }
    if db.prepare("SELECT EXISTS(SELECT 1 FROM pragma_table_info('database_profiles') WHERE name='password_target')").await?.query_row(()).await?.get::<bool>(0)? {
        crate::store::databases::credentials::validate(db).await.map_err(|error|Error::Worker(error.message))?;
    }
    let live = if archives {
        String::new()
    } else {
        format!(" WHERE turn IN ({LIVE_TURNS})")
    };
    let revisions = if archives {
        String::new()
    } else {
        format!(
            " WHERE (session,revision) IN (SELECT id,revision FROM sessions WHERE state!='removed' UNION SELECT session,revision FROM turns WHERE id IN ({LIVE_TURNS}))"
        )
    };
    if db
        .prepare("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='network_settings')")
        .await?
        .query_row(())
        .await?
        .get::<bool>(0)?
    {
        records::<crate::store::network::Stored>(
            db,
            "network_settings.body",
            "SELECT body FROM network_settings",
        )
        .await?;
        crate::store::network::validate_stored(db)
            .await
            .map_err(|_| Error::Worker("incompatible stored network configuration".into()))?;
    }
    records::<sailry_protocol::artifact::Summary>(
        db,
        "artifacts.body",
        "SELECT body FROM artifacts",
    )
    .await?;
    if archives {
        records::<sailry_protocol::artifact::Summary>(
            db,
            "artifact_revisions.body",
            "SELECT body FROM artifact_revisions",
        )
        .await?;
        crate::store::artifacts::validate_stored(db).await
    } else {
        crate::store::artifacts::validate_current(db).await
    }
    .map_err(|error| Error::Worker(error.message))?;
    records::<crate::store::backup::Stored>(
        db,
        "backup_settings.body",
        "SELECT body FROM backup_settings",
    )
    .await?;
    crate::store::backup::read(db)
        .await
        .map_err(|_| Error::Worker("incompatible stored backup configuration".into()))?;
    records::<crate::store::search::Stored>(
        db,
        "search_settings.body",
        "SELECT body FROM search_settings",
    )
    .await?;
    crate::store::search::validate_stored(db)
        .await
        .map_err(|_| {
            Error::Worker(
                "incompatible stored search configuration; existing data was not changed".into(),
            )
        })?;
    records::<sailry_protocol::sandbox::Settings>(
        db,
        "sandbox_settings.body",
        "SELECT body FROM sandbox_settings",
    )
    .await?;
    records::<sailry_protocol::models::Settings>(
        db,
        "local_model_settings.body",
        "SELECT body FROM local_model_settings",
    )
    .await?;
    crate::store::local_models::read(db).await.map_err(|_| {
        Error::Worker(
            "incompatible stored local model configuration; existing data was not changed".into(),
        )
    })?;
    records::<crate::store::knowledge::Stored>(
        db,
        "knowledge_collections.body",
        "SELECT body FROM knowledge_collections",
    )
    .await?;
    crate::store::knowledge::validate_stored(db)
        .await
        .map_err(|_| {
            Error::Worker(
                "incompatible stored knowledge configuration; existing data was not changed".into(),
            )
        })?;
    records::<role::Profile>(db, "roles.body", "SELECT body FROM roles").await?;
    records::<sailry_protocol::hooks::Settings>(
        db,
        "hook_settings.body",
        "SELECT body FROM hook_settings",
    )
    .await?;
    records::<sailry_protocol::hooks::Settings>(
        db,
        "agent_runs.hooks",
        &format!("SELECT hooks FROM agent_runs{live}"),
    )
    .await?;
    records::<Result<sailry_protocol::process::Outcome, Fault>>(
        db,
        "hook_executions.result",
        &format!(
            "SELECT result FROM hook_executions WHERE result IS NOT NULL{}",
            if archives {
                String::new()
            } else {
                format!(" AND turn IN ({LIVE_TURNS})")
            }
        ),
    )
    .await?;
    records::<sailry_protocol::code_index::Settings>(
        db,
        "code_index_settings.body",
        "SELECT body FROM code_index_settings",
    )
    .await?;
    records::<sailry_protocol::code::Settings>(
        db,
        "code_settings.body",
        "SELECT body FROM code_settings",
    )
    .await?;

    records::<sailry_protocol::code::Startup>(
        db,
        "code_startups.body",
        "SELECT body FROM code_startups",
    )
    .await?;
    records::<sailry_protocol::conversation::Approval>(
        db,
        "agent_approvals.body",
        &format!("SELECT body FROM agent_approvals{live}"),
    )
    .await?;
    records::<Option<crate::store::agent::ApprovalBinding>>(
        db,
        "agent_approvals.binding",
        &format!("SELECT binding FROM agent_approvals{live}"),
    )
    .await?;
    records::<Option<crate::store::agent::approvals::Prepared>>(
        db,
        "agent_approvals.prepared",
        &format!("SELECT prepared FROM agent_approvals{live}"),
    )
    .await?;
    records::<crate::store::agent::ApprovalBinding>(
        db,
        "agent_approval_rules.binding",
        "SELECT binding FROM agent_approval_rules",
    )
    .await?;
    records::<sailry_protocol::conversation::ApprovalChoice>(
        db,
        "agent_approval_rules.choice",
        "SELECT choice FROM agent_approval_rules",
    )
    .await?;
    records::<sailry_protocol::notification::Notice>(
        db,
        "notifications.body",
        "SELECT body FROM notifications",
    )
    .await?;
    records::<sailry_protocol::dispatch::Handler>(
        db,
        "dispatch_handlers.body",
        "SELECT body FROM dispatch_handlers",
    )
    .await?;
    records::<sailry_protocol::dispatch::Schedule>(
        db,
        "dispatch_schedules.body",
        "SELECT body FROM dispatch_schedules",
    )
    .await?;
    records::<sailry_protocol::dispatch::Job>(
        db,
        "dispatch_jobs.body",
        if archives {
            "SELECT body FROM dispatch_jobs"
        } else {
            "SELECT body FROM dispatch_jobs WHERE status='queued' UNION ALL SELECT body FROM dispatch_jobs WHERE status='running'"
        },
    )
    .await?;
    records::<sailry_protocol::dispatch::Callback>(
        db,
        "dispatch_jobs.callback",
        if archives {
            "SELECT callback FROM dispatch_jobs"
        } else {
            "SELECT callback FROM dispatch_jobs WHERE status='queued' UNION ALL SELECT callback FROM dispatch_jobs WHERE status='running'"
        },
    )
    .await?;
    if archives {
        records::<sailry_protocol::dispatch::Event>(
            db,
            "dispatch_events.body",
            "SELECT body FROM dispatch_events",
        )
        .await?;
    }
    records::<sailry_protocol::plugin::Info>(db, "plugins.body", "SELECT body FROM plugins")
        .await?;
    records::<sailry_protocol::plugin::Info>(
        db,
        "plugin_packages.body",
        if archives {
            "SELECT body FROM plugin_packages"
        } else {
            "SELECT p.body FROM plugin_packages p JOIN plugins c ON json_extract(c.body,'$.summary.digest')=p.digest WHERE c.installed=1"
        },
    ).await?;
    records::<crate::store::plugins::settings::Stored>(
        db,
        "plugin_settings.body",
        if archives {
            "SELECT body FROM plugin_settings"
        } else {
            "SELECT p.body FROM plugin_settings p JOIN plugins c ON c.name=p.plugin AND json_extract(c.body,'$.summary.settings_revision')=p.revision WHERE c.installed=1"
        },
    ).await?;
    for record in crate::store::plugins::connectors::records(db)
        .await
        .map_err(|error| crate::Error::Worker(error.message))?
    {
        crate::store::plugins::connectors::validate(&record)
            .map_err(|error| crate::Error::Worker(error.message))?;
    }
    records::<crate::store::agent::background::Record>(
        db,
        "agent_background.body",
        if archives {
            "SELECT body FROM agent_background"
        } else {
            "SELECT body FROM agent_background WHERE state='running' UNION ALL SELECT body FROM agent_background WHERE state='pending'"
        },
    )
    .await?;
    records::<crate::store::background_settings::Stored>(
        db,
        "background_settings.body",
        "SELECT body FROM background_settings",
    )
    .await?;
    crate::store::background_settings::retained(db)
        .await
        .map_err(|_| Error::Worker("incompatible stored background settings".into()))?;
    records::<crate::store::agent::continuation::Fence>(
        db,
        "agent_continuations.body",
        &format!("SELECT body FROM agent_continuations{live}"),
    )
    .await?;
    records::<super::super::providers::Stored>(db, "providers.body", "SELECT body FROM providers")
        .await?;
    super::super::providers::validate_stored(db)
        .await
        .map_err(|_| {
            Error::Worker(
                "incompatible stored provider configuration; existing data was not changed".into(),
            )
        })?;
    records::<SessionConfig>(db, "defaults.config", "SELECT config FROM defaults").await?;
    records::<SessionConfig>(
        db,
        "session_revisions.config",
        &format!("SELECT config FROM session_revisions{revisions}"),
    )
    .await?;
    records::<Vec<sailry_protocol::plugin::Reference>>(
        db,
        "turns.plugins",
        &format!(
            "SELECT plugins FROM turns{}",
            live.replacen("turn IN", "id IN", 1)
        ),
    )
    .await?;
    records::<Vec<crate::store::captures::Instruction>>(
        db,
        "turns.instructions",
        &format!(
            "SELECT instructions FROM turns{}",
            live.replacen("turn IN", "id IN", 1)
        ),
    )
    .await?;
    records::<role::Snapshot>(
        db,
        "turns.roles",
        &format!(
            "SELECT roles FROM turns WHERE roles IS NOT NULL{}",
            live.replacen(" WHERE ", " AND ", 1)
                .replacen("turn IN", "id IN", 1)
        ),
    )
    .await?;
    records::<Provider>(
        db,
        "agent_runs.provider",
        &format!(
            "SELECT provider FROM agent_runs WHERE provider IS NOT NULL{}",
            live.replacen(" WHERE ", " AND ", 1)
        ),
    )
    .await?;
    records::<sailry_protocol::conversation::Input>(
        db,
        "agent_runs.input",
        &format!(
            "SELECT input FROM agent_runs WHERE input IS NOT NULL{}",
            live.replacen(" WHERE ", " AND ", 1)
        ),
    )
    .await?;
    records::<SessionProfile>(
        db,
        "session_revisions.profile",
        &format!(
            "SELECT profile FROM session_revisions WHERE profile IS NOT NULL{}",
            revisions.replacen(" WHERE ", " AND ", 1)
        ),
    )
    .await?;
    records::<role::Snapshot>(
        db,
        "session_revisions.roles",
        &format!("SELECT roles FROM session_revisions{revisions}"),
    )
    .await?;
    records::<crate::store::media::Snapshot>(
        db,
        "session_media.body",
        "SELECT body FROM session_media",
    )
    .await?;
    records::<sailry_protocol::media::Settings>(
        db,
        "media_settings.body",
        "SELECT body FROM media_settings",
    )
    .await?;
    records::<Event>(
        db,
        "events.body",
        if archives {
            "SELECT body FROM events"
        } else {
            "SELECT body FROM events WHERE json_extract(body,'$.kind')='sessions_reordered' ORDER BY cursor DESC LIMIT 1"
        },
    ).await?;
    records::<Result<Output, Fault>>(
        db,
        "requests.result",
        if archives {
            "SELECT CAST(result AS BLOB) FROM requests WHERE result IS NOT NULL"
        } else {
            "SELECT CAST(result AS BLOB) FROM requests WHERE result IS NOT NULL AND status='admitted' UNION ALL SELECT CAST(result AS BLOB) FROM requests WHERE result IS NOT NULL AND status='unknown'"
        },
    ).await?;
    records::<sailry_protocol::conversation::Run>(
        db,
        "agent_runs.body",
        &format!("SELECT body FROM agent_runs{live}"),
    )
    .await?;
    records::<sailry_protocol::conversation::question::Question>(
        db,
        "agent_questions.body",
        &format!("SELECT body FROM agent_questions{live}"),
    )
    .await?;
    records::<sailry_protocol::Request>(
        db,
        "requests.body",
        "SELECT r.body FROM agent_pending p JOIN requests r ON r.caller=p.caller AND r.id=p.request",
    ).await?;
    if archives {
        records::<adk_core::Event>(db, "agent_events.body", "SELECT body FROM agent_events")
            .await?;
    } else {
        // These completed receipts can participate in recovery, unlike unused archives.
        records::<Result<Output, Fault>>(
            db,
            "requests.result",
            &format!(
                "SELECT CAST(result AS BLOB) FROM requests WHERE result IS NOT NULL AND (caller,id) IN (SELECT caller,request FROM turns WHERE id IN ({LIVE_TURNS}) UNION SELECT caller,request FROM agent_pending UNION SELECT n.identity,json_extract(j.body,'$.request') FROM node n JOIN dispatch_jobs j ON j.status='running')"
            ),
        ).await?;
    }
    super::counters::validate(db).await?;
    Ok(())
}

pub(in crate::store) async fn records<T: DeserializeOwned>(
    db: &Connection,
    source: &str,
    sql: &str,
) -> Result<(), Error> {
    let mut rows = db.query(sql, ()).await?;
    let mut index = 0;
    while let Some(row) = rows.next().await? {
        let Value::Blob(body) = row.get_value(0)? else {
            return Err(Error::Worker(format!(
                "invalid stored JSON type in {source}, record {}; existing data was not changed",
                index + 1
            )));
        };
        serde_json::from_slice::<T>(&body).map_err(|error| {
            // Serde messages can include invalid values. Report the location, not the payload.
            Error::Worker(format!(
                "invalid stored JSON in {source}, record {}, line {}, column {} ({:?}); existing data was not changed",
                index + 1, error.line(), error.column(), error.classify(),
            ))
        })?;
        index += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
