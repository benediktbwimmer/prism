use std::collections::HashMap;

use anyhow::Result;
use prism_projections::MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE;
use rusqlite::{params, Connection, Transaction};

pub(super) fn load_projection_snapshot_rows(
    conn: &Connection,
) -> Result<Option<prism_projections::ProjectionSnapshot>> {
    let mut co_change_by_lineage =
        HashMap::<prism_ir::LineageId, Vec<prism_projections::CoChangeRecord>>::new();
    {
        let mut stmt = conn.prepare(
            "SELECT source_lineage, target_lineage, count
             FROM (
                 SELECT source_lineage, target_lineage, count,
                        ROW_NUMBER() OVER (
                            PARTITION BY source_lineage
                            ORDER BY count DESC, target_lineage
                        ) AS rank
                 FROM projection_co_change
             )
             WHERE rank <= ?1
             ORDER BY source_lineage, count DESC, target_lineage",
        )?;
        let rows = stmt.query_map([MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE as i64], |row| {
            Ok((
                prism_ir::LineageId::new(row.get::<_, String>(0)?),
                prism_projections::CoChangeRecord {
                    lineage: prism_ir::LineageId::new(row.get::<_, String>(1)?),
                    count: row.get::<_, u32>(2)?,
                },
            ))
        })?;
        for row in rows {
            let (source, record) = row?;
            co_change_by_lineage.entry(source).or_default().push(record);
        }
    }

    let mut validation_by_lineage =
        HashMap::<prism_ir::LineageId, Vec<prism_projections::ValidationCheck>>::new();
    {
        let mut stmt = conn.prepare(
            "SELECT lineage, label, score, last_seen
             FROM projection_validation
             ORDER BY lineage, score DESC, last_seen DESC, label",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                prism_ir::LineageId::new(row.get::<_, String>(0)?),
                prism_projections::ValidationCheck {
                    label: row.get(1)?,
                    score: row.get(2)?,
                    last_seen: row.get::<_, i64>(3)? as u64,
                },
            ))
        })?;
        for row in rows {
            let (lineage, check) = row?;
            validation_by_lineage
                .entry(lineage)
                .or_default()
                .push(check);
        }
    }

    if co_change_by_lineage.is_empty() && validation_by_lineage.is_empty() {
        return Ok(None);
    }

    let mut co_change_by_lineage = co_change_by_lineage.into_iter().collect::<Vec<_>>();
    co_change_by_lineage.sort_by(|left, right| left.0 .0.cmp(&right.0 .0));
    let mut validation_by_lineage = validation_by_lineage.into_iter().collect::<Vec<_>>();
    validation_by_lineage.sort_by(|left, right| left.0 .0.cmp(&right.0 .0));

    Ok(Some(prism_projections::ProjectionSnapshot {
        co_change_by_lineage,
        validation_by_lineage,
    }))
}

pub(super) fn save_projection_snapshot_tx(
    tx: &Transaction<'_>,
    snapshot: &prism_projections::ProjectionSnapshot,
) -> Result<()> {
    tx.execute("DELETE FROM projection_co_change", [])?;
    tx.execute("DELETE FROM projection_validation", [])?;

    for (source, neighbors) in &snapshot.co_change_by_lineage {
        for record in neighbors {
            tx.execute(
                "INSERT INTO projection_co_change(source_lineage, target_lineage, count)
                 VALUES (?1, ?2, ?3)",
                params![source.0.as_str(), record.lineage.0.as_str(), record.count],
            )?;
        }
    }
    prune_projection_co_change_tx(tx)?;

    for (lineage, checks) in &snapshot.validation_by_lineage {
        for check in checks {
            tx.execute(
                "INSERT INTO projection_validation(lineage, label, score, last_seen)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    lineage.0.as_str(),
                    check.label.as_str(),
                    check.score,
                    check.last_seen as i64
                ],
            )?;
        }
    }

    Ok(())
}

pub(super) fn prune_projection_co_change(conn: &mut Connection) -> Result<()> {
    let tx = conn.transaction()?;
    prune_projection_co_change_tx(&tx)?;
    tx.commit()?;
    Ok(())
}

pub(super) fn apply_projection_co_change_deltas_tx(
    tx: &Transaction<'_>,
    deltas: &[prism_projections::CoChangeDelta],
) -> Result<()> {
    for delta in deltas {
        tx.execute(
            "INSERT INTO projection_co_change(source_lineage, target_lineage, count)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(source_lineage, target_lineage)
             DO UPDATE SET count = projection_co_change.count + excluded.count",
            params![
                delta.source_lineage.0.as_str(),
                delta.target_lineage.0.as_str(),
                delta.count_delta
            ],
        )?;
    }
    if !deltas.is_empty() {
        prune_projection_co_change_tx(tx)?;
    }
    Ok(())
}

fn prune_projection_co_change_tx(tx: &Transaction<'_>) -> Result<()> {
    tx.execute(
        "DELETE FROM projection_co_change
         WHERE (source_lineage, target_lineage) IN (
             SELECT source_lineage, target_lineage
             FROM (
                 SELECT source_lineage, target_lineage,
                        ROW_NUMBER() OVER (
                            PARTITION BY source_lineage
                            ORDER BY count DESC, target_lineage
                        ) AS rank
                 FROM projection_co_change
             )
             WHERE rank > ?1
         )",
        params![MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE as i64],
    )?;
    Ok(())
}

pub(super) fn apply_projection_validation_deltas_tx(
    tx: &Transaction<'_>,
    deltas: &[prism_projections::ValidationDelta],
) -> Result<()> {
    for delta in deltas {
        tx.execute(
            "INSERT INTO projection_validation(lineage, label, score, last_seen)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(lineage, label)
             DO UPDATE SET
               score = projection_validation.score + excluded.score,
               last_seen = MAX(projection_validation.last_seen, excluded.last_seen)",
            params![
                delta.lineage.0.as_str(),
                delta.label.as_str(),
                delta.score_delta,
                delta.last_seen as i64
            ],
        )?;
    }
    Ok(())
}
