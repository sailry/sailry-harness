//! Preserve previously expanded instructions without changing immutable captures.
use crate::Error;
use crate::store::captures::Instruction;
use rusqlite::{Connection, OptionalExtension, params};
use sailry_protocol::{plugin, tool::Operation};

// Source data from the retired coordinator prompt; never selected by the runner.
const RETAINED: &str = "Proactive delegation\nActively identify useful, self-contained research, implementation or review work that an available child agent can handle independently. Delegate when this improves progress or provides an independent check; keep the critical dependent work moving yourself. Give each child a clear scope and expected result, separate overlapping file ownership, and reuse or guide existing children instead of duplicating their work. Background children report back as a background notice when they finish; verify and incorporate their results then. Handle small or tightly coupled tasks directly; do not split work merely to create agents or require delegation on every turn. Use only admitted tools and roles within the current work mode and permissions. Children must not delegate further.";

pub(super) fn reconcile(db: &Connection) -> Result<bool, Error> {
    let mut changed = false;
    let mut after = i64::MIN;
    loop {
        let row = db.query_row(
            "SELECT t.rowid,t.id,t.plugins FROM turns t JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision WHERE t.rowid>=?1 AND t.instructions=X'5B5D' AND json_extract(r.config,'$.delegation_proactive')=1 ORDER BY t.rowid LIMIT 1",
            [after], |row| Ok((row.get::<_,i64>(0)?,row.get::<_,String>(1)?,row.get::<_,Vec<u8>>(2)?)),
        ).optional()?;
        let Some((row, turn, bytes)) = row else { break };
        let references: Vec<plugin::Reference> =
            serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        let packages =
            crate::store::agent::plugins::packages(db, &references).map_err(|_| invalid())?;
        let tools = packages
            .iter()
            .flat_map(|info| {
                info.extension.iter().flat_map(move |extension| {
                    extension
                        .tools
                        .iter()
                        .filter(|tool| {
                            tool.operation.or_else(|| {
                                tool.handler.as_ref().and_then(|handler| handler.operation)
                            }) == Some(Operation::DelegateAgent)
                        })
                        .map(move |tool| {
                            crate::plugins::tools::alias(&info.summary.name, &tool.name)
                        })
                })
            })
            .collect::<Vec<_>>();
        if !tools.is_empty() {
            let instructions = vec![Instruction {
                text: RETAINED.into(),
                any_tools: tools,
            }];
            let bytes = serde_json::to_vec(&instructions).map_err(|_| invalid())?;
            db.execute(
                "UPDATE turns SET instructions=?2 WHERE id=?1",
                params![turn, bytes],
            )?;
            changed = true;
        }
        let Some(next) = row.checked_add(1) else {
            break;
        };
        after = next;
    }
    Ok(changed)
}

fn invalid() -> Error {
    Error::Worker("stored instruction capture is invalid".into())
}

#[cfg(test)]
mod tests;
