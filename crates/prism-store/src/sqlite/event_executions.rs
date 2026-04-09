use anyhow::{Context, Result};
use prism_coordination::EventExecutionRecord;
use prism_ir::{EventExecutionId, EventExecutionStatus, EventTriggerKind};
use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::store::EventExecutionRecordQuery;

pub(super) fn upsert_event_execution_record_tx(
    tx: &Transaction<'_>,
    record: &EventExecutionRecord,
) -> Result<()> {
    let payload =
        serde_json::to_string(record).context("failed to serialize event execution record")?;
    tx.execute(
        "INSERT INTO event_execution_record(
            id,
            trigger_kind,
            status,
            authoritative_revision,
            claimed_at,
            started_at,
            finished_at,
            expires_at,
            payload
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        ON CONFLICT(id) DO UPDATE SET
            trigger_kind = excluded.trigger_kind,
            status = excluded.status,
            authoritative_revision = excluded.authoritative_revision,
            claimed_at = excluded.claimed_at,
            started_at = excluded.started_at,
            finished_at = excluded.finished_at,
            expires_at = excluded.expires_at,
            payload = excluded.payload",
        params![
            record.id.0.as_str(),
            encode_trigger_kind(record.trigger_kind),
            encode_status(record.status),
            record.authoritative_revision.map(|value| value as i64),
            record.claimed_at as i64,
            record.started_at.map(|value| value as i64),
            record.finished_at.map(|value| value as i64),
            record.expires_at.map(|value| value as i64),
            payload,
        ],
    )?;
    Ok(())
}

pub(super) fn load_event_execution_record(
    conn: &Connection,
    event_execution_id: &EventExecutionId,
) -> Result<Option<EventExecutionRecord>> {
    conn.query_row(
        "SELECT payload
         FROM event_execution_record
         WHERE id = ?1",
        params![event_execution_id.0.as_str()],
        |row| row.get::<_, String>(0),
    )
    .optional()?
    .map(|payload| decode_record(&payload))
    .transpose()
}

pub(super) fn load_event_execution_records(
    conn: &Connection,
    query: &EventExecutionRecordQuery,
) -> Result<Vec<EventExecutionRecord>> {
    if let Some(limit) = query.limit {
        let mut statement = conn.prepare(
            "SELECT payload
             FROM event_execution_record
             ORDER BY claimed_at DESC, id ASC
             LIMIT ?1",
        )?;
        let payloads = statement
            .query_map(params![limit as i64], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        return payloads
            .into_iter()
            .map(|payload| decode_record(&payload))
            .collect();
    }

    let mut statement = conn.prepare(
        "SELECT payload
         FROM event_execution_record
         ORDER BY claimed_at DESC, id ASC",
    )?;
    let payloads = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    payloads
        .into_iter()
        .map(|payload| decode_record(&payload))
        .collect()
}

fn decode_record(payload: &str) -> Result<EventExecutionRecord> {
    serde_json::from_str(payload).context("failed to decode event execution record payload")
}

fn encode_status(status: EventExecutionStatus) -> &'static str {
    match status {
        EventExecutionStatus::Claimed => "claimed",
        EventExecutionStatus::Running => "running",
        EventExecutionStatus::Succeeded => "succeeded",
        EventExecutionStatus::Failed => "failed",
        EventExecutionStatus::Expired => "expired",
        EventExecutionStatus::Abandoned => "abandoned",
    }
}

fn encode_trigger_kind(kind: EventTriggerKind) -> &'static str {
    match kind {
        EventTriggerKind::TaskBecameActionable => "task_became_actionable",
        EventTriggerKind::ClaimExpired => "claim_expired",
        EventTriggerKind::RecurringPlanTick => "recurring_plan_tick",
        EventTriggerKind::RuntimeBecameStale => "runtime_became_stale",
        EventTriggerKind::HookRequested => "hook_requested",
    }
}
