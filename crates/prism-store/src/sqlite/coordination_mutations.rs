use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::store::CoordinationPersistContext;

pub(super) fn append_mutation_tx(
    tx: &Transaction<'_>,
    revision: u64,
    expected_revision: Option<u64>,
    inserted_events: usize,
    applied: bool,
    context: &CoordinationPersistContext,
) -> Result<()> {
    tx.execute(
        "INSERT INTO coordination_mutation_log(
            revision,
            expected_revision,
            inserted_events,
            applied,
            repo_id,
            worktree_id,
            branch_ref,
            session_id,
            instance_id
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            revision as i64,
            expected_revision.map(|value| value as i64),
            inserted_events as i64,
            applied as i64,
            context.repo_id,
            context.worktree_id,
            context.branch_ref,
            context.session_id,
            context.instance_id,
        ],
    )?;
    Ok(())
}

pub(super) fn load_latest_context(conn: &Connection) -> Result<Option<CoordinationPersistContext>> {
    conn.query_row(
        "SELECT repo_id, worktree_id, branch_ref, session_id, instance_id
         FROM coordination_mutation_log
         ORDER BY sequence DESC
         LIMIT 1",
        [],
        |row| {
            Ok(CoordinationPersistContext {
                repo_id: row.get(0)?,
                worktree_id: row.get(1)?,
                branch_ref: row.get(2)?,
                session_id: row.get(3)?,
                instance_id: row.get(4)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}
