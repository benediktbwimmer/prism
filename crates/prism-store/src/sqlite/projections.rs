use std::collections::{BTreeSet, HashMap};
use std::time::Instant;

use anyhow::Result;
use prism_projections::MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE;
use rusqlite::{params, Connection, Transaction};
use tracing::info;

const PROJECTION_CO_CHANGE_PRUNED_ON_OPEN_KEY: &str = "projection:co_change_pruned_on_open";
const PROJECTION_HAS_CO_CHANGE_KEY: &str = "projection:has_co_change";
const PROJECTION_HAS_VALIDATION_KEY: &str = "projection:has_validation";
const PROJECTION_HAS_KNOWLEDGE_KEY: &str = "projection:has_knowledge";

pub(super) fn load_projection_materialization_metadata(
    conn: &Connection,
) -> Result<crate::ProjectionMaterializationMetadata> {
    Ok(crate::ProjectionMaterializationMetadata {
        has_co_change: metadata_or_row_presence(
            conn,
            PROJECTION_HAS_CO_CHANGE_KEY,
            "SELECT EXISTS(SELECT 1 FROM projection_co_change LIMIT 1)",
        )?,
        has_validation: metadata_or_row_presence(
            conn,
            PROJECTION_HAS_VALIDATION_KEY,
            "SELECT EXISTS(SELECT 1 FROM projection_validation LIMIT 1)",
        )?,
        has_knowledge: metadata_or_row_presence(
            conn,
            PROJECTION_HAS_KNOWLEDGE_KEY,
            "SELECT EXISTS(
                SELECT 1 FROM projection_curated_concept LIMIT 1
            ) OR EXISTS(
                SELECT 1 FROM projection_concept_relation LIMIT 1
            )",
        )?,
    })
}

pub(super) fn load_projection_snapshot_rows(
    conn: &Connection,
) -> Result<Option<prism_projections::ProjectionSnapshot>> {
    let started = Instant::now();
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

    let mut curated_concepts = Vec::<prism_projections::ConceptPacket>::new();
    {
        let mut stmt = conn.prepare(
            "SELECT payload
             FROM projection_curated_concept
             ORDER BY handle",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        for row in rows {
            curated_concepts.push(serde_json::from_str(&row?)?);
        }
    }

    let mut concept_relations = Vec::<prism_projections::ConceptRelation>::new();
    {
        let mut stmt = conn.prepare(
            "SELECT payload
             FROM projection_concept_relation
             ORDER BY source_handle, target_handle, kind",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        for row in rows {
            concept_relations.push(serde_json::from_str(&row?)?);
        }
    }

    if co_change_by_lineage.is_empty()
        && validation_by_lineage.is_empty()
        && curated_concepts.is_empty()
        && concept_relations.is_empty()
    {
        info!(
            total_ms = started.elapsed().as_millis(),
            "loaded prism projection snapshot: none"
        );
        return Ok(None);
    }

    let co_change_lineages = co_change_by_lineage.len();
    let co_change_records = co_change_by_lineage.values().map(Vec::len).sum::<usize>();
    let validation_lineages = validation_by_lineage.len();
    let validation_records = validation_by_lineage.values().map(Vec::len).sum::<usize>();
    let curated_count = curated_concepts.len();
    let relation_count = concept_relations.len();

    let mut co_change_by_lineage = co_change_by_lineage.into_iter().collect::<Vec<_>>();
    co_change_by_lineage.sort_by(|left, right| left.0 .0.cmp(&right.0 .0));
    let mut validation_by_lineage = validation_by_lineage.into_iter().collect::<Vec<_>>();
    validation_by_lineage.sort_by(|left, right| left.0 .0.cmp(&right.0 .0));
    curated_concepts.sort_by(|left, right| left.handle.cmp(&right.handle));

    info!(
        co_change_lineages,
        co_change_records,
        validation_lineages,
        validation_records,
        curated_count,
        relation_count,
        total_ms = started.elapsed().as_millis(),
        "loaded prism projection snapshot"
    );

    Ok(Some(prism_projections::ProjectionSnapshot {
        co_change_by_lineage,
        validation_by_lineage,
        curated_concepts,
        concept_relations,
    }))
}

pub(super) fn has_derived_projection_rows(conn: &Connection) -> Result<bool> {
    let has_co_change = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM projection_co_change LIMIT 1)",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    if has_co_change != 0 {
        return Ok(true);
    }
    let has_validation = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM projection_validation LIMIT 1)",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    Ok(has_validation != 0)
}

pub(super) fn load_projection_knowledge_rows(
    conn: &Connection,
) -> Result<Option<prism_projections::ProjectionSnapshot>> {
    let started = Instant::now();

    let mut curated_concepts = Vec::<prism_projections::ConceptPacket>::new();
    {
        let mut stmt = conn.prepare(
            "SELECT payload
             FROM projection_curated_concept
             ORDER BY handle",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        for row in rows {
            curated_concepts.push(serde_json::from_str(&row?)?);
        }
    }

    let mut concept_relations = Vec::<prism_projections::ConceptRelation>::new();
    {
        let mut stmt = conn.prepare(
            "SELECT payload
             FROM projection_concept_relation
             ORDER BY source_handle, target_handle, kind",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        for row in rows {
            concept_relations.push(serde_json::from_str(&row?)?);
        }
    }

    if curated_concepts.is_empty() && concept_relations.is_empty() {
        info!(
            total_ms = started.elapsed().as_millis(),
            "loaded prism projection knowledge snapshot: none"
        );
        return Ok(None);
    }

    let curated_count = curated_concepts.len();
    let relation_count = concept_relations.len();
    curated_concepts.sort_by(|left, right| left.handle.cmp(&right.handle));

    info!(
        curated_count,
        relation_count,
        total_ms = started.elapsed().as_millis(),
        "loaded prism projection knowledge snapshot"
    );

    Ok(Some(prism_projections::ProjectionSnapshot {
        co_change_by_lineage: Vec::new(),
        validation_by_lineage: Vec::new(),
        curated_concepts,
        concept_relations,
    }))
}

pub(super) fn load_projection_snapshot_without_co_change_rows(
    conn: &Connection,
) -> Result<Option<prism_projections::ProjectionSnapshot>> {
    let started = Instant::now();

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

    let mut curated_concepts = Vec::<prism_projections::ConceptPacket>::new();
    {
        let mut stmt = conn.prepare(
            "SELECT payload
             FROM projection_curated_concept
             ORDER BY handle",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        for row in rows {
            curated_concepts.push(serde_json::from_str(&row?)?);
        }
    }

    let mut concept_relations = Vec::<prism_projections::ConceptRelation>::new();
    {
        let mut stmt = conn.prepare(
            "SELECT payload
             FROM projection_concept_relation
             ORDER BY source_handle, target_handle, kind",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        for row in rows {
            concept_relations.push(serde_json::from_str(&row?)?);
        }
    }

    if validation_by_lineage.is_empty()
        && curated_concepts.is_empty()
        && concept_relations.is_empty()
    {
        info!(
            total_ms = started.elapsed().as_millis(),
            "loaded prism projection snapshot without co-change: none"
        );
        return Ok(None);
    }

    let validation_lineages = validation_by_lineage.len();
    let validation_records = validation_by_lineage.values().map(Vec::len).sum::<usize>();
    let curated_count = curated_concepts.len();
    let relation_count = concept_relations.len();
    let mut validation_by_lineage = validation_by_lineage.into_iter().collect::<Vec<_>>();
    validation_by_lineage.sort_by(|left, right| left.0 .0.cmp(&right.0 .0));
    curated_concepts.sort_by(|left, right| left.handle.cmp(&right.handle));

    info!(
        validation_lineages,
        validation_records,
        curated_count,
        relation_count,
        total_ms = started.elapsed().as_millis(),
        "loaded prism projection snapshot without co-change"
    );

    Ok(Some(prism_projections::ProjectionSnapshot {
        co_change_by_lineage: Vec::new(),
        validation_by_lineage,
        curated_concepts,
        concept_relations,
    }))
}

pub(super) fn save_projection_snapshot_tx(
    tx: &Transaction<'_>,
    snapshot: &prism_projections::ProjectionSnapshot,
) -> Result<()> {
    tx.execute("DELETE FROM projection_co_change", [])?;
    tx.execute("DELETE FROM projection_validation", [])?;
    tx.execute("DELETE FROM projection_curated_concept", [])?;
    tx.execute("DELETE FROM projection_concept_relation", [])?;

    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO projection_co_change(source_lineage, target_lineage, count)
             VALUES (?1, ?2, ?3)",
        )?;
        for (source, record) in normalized_co_change_rows(snapshot) {
            stmt.execute(params![
                source.0.as_str(),
                record.lineage.0.as_str(),
                record.count
            ])?;
        }
    }

    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO projection_validation(lineage, label, score, last_seen)
             VALUES (?1, ?2, ?3, ?4)",
        )?;
        for (lineage, checks) in &snapshot.validation_by_lineage {
            for check in checks {
                stmt.execute(params![
                    lineage.0.as_str(),
                    check.label.as_str(),
                    check.score,
                    check.last_seen as i64
                ])?;
            }
        }
    }

    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO projection_curated_concept(handle, payload)
             VALUES (?1, ?2)",
        )?;
        for concept in &snapshot.curated_concepts {
            stmt.execute(params![
                concept.handle.as_str(),
                serde_json::to_string(concept)?
            ])?;
        }
    }

    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO projection_concept_relation(source_handle, target_handle, kind, payload)
             VALUES (?1, ?2, ?3, ?4)",
        )?;
        for relation in &snapshot.concept_relations {
            stmt.execute(params![
                relation.source_handle.as_str(),
                relation.target_handle.as_str(),
                serde_json::to_string(&relation.kind)?,
                serde_json::to_string(relation)?,
            ])?;
        }
    }

    set_projection_co_change_pruned_on_open_tx(tx)?;
    set_projection_materialization_metadata_tx(
        tx,
        crate::ProjectionMaterializationMetadata {
            has_co_change: !snapshot.co_change_by_lineage.is_empty(),
            has_validation: !snapshot.validation_by_lineage.is_empty(),
            has_knowledge: !snapshot.curated_concepts.is_empty()
                || !snapshot.concept_relations.is_empty(),
        },
    )?;

    Ok(())
}

pub(super) fn upsert_curated_concept_tx(
    tx: &Transaction<'_>,
    concept: &prism_projections::ConceptPacket,
) -> Result<usize> {
    let changed = tx.execute(
        "INSERT INTO projection_curated_concept(handle, payload)
         VALUES (?1, ?2)
         ON CONFLICT(handle) DO UPDATE SET payload = excluded.payload",
        params![concept.handle.as_str(), serde_json::to_string(concept)?],
    )?;
    set_projection_materialization_flag_tx(tx, PROJECTION_HAS_KNOWLEDGE_KEY, true)?;
    Ok(changed)
}

pub(super) fn delete_curated_concept_tx(tx: &Transaction<'_>, handle: &str) -> Result<usize> {
    let changed = tx.execute(
        "DELETE FROM projection_curated_concept WHERE handle = ?1",
        [handle],
    )?;
    update_projection_knowledge_flag_tx(tx)?;
    Ok(changed)
}

pub(super) fn upsert_concept_relation_tx(
    tx: &Transaction<'_>,
    relation: &prism_projections::ConceptRelation,
) -> Result<usize> {
    let changed = tx.execute(
        "INSERT INTO projection_concept_relation(source_handle, target_handle, kind, payload)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(source_handle, target_handle, kind)
         DO UPDATE SET payload = excluded.payload",
        params![
            relation.source_handle.as_str(),
            relation.target_handle.as_str(),
            serde_json::to_string(&relation.kind)?,
            serde_json::to_string(relation)?,
        ],
    )?;
    set_projection_materialization_flag_tx(tx, PROJECTION_HAS_KNOWLEDGE_KEY, true)?;
    Ok(changed)
}

pub(super) fn delete_concept_relation_tx(
    tx: &Transaction<'_>,
    source_handle: &str,
    target_handle: &str,
    kind: prism_projections::ConceptRelationKind,
) -> Result<usize> {
    let changed = tx.execute(
        "DELETE FROM projection_concept_relation
         WHERE source_handle = ?1 AND target_handle = ?2 AND kind = ?3",
        params![source_handle, target_handle, serde_json::to_string(&kind)?],
    )?;
    update_projection_knowledge_flag_tx(tx)?;
    Ok(changed)
}

pub(super) fn prune_projection_co_change(conn: &mut Connection) -> Result<usize> {
    if super::metadata_value(conn, PROJECTION_CO_CHANGE_PRUNED_ON_OPEN_KEY)? > 0 {
        return Ok(0);
    }

    let has_rows = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM projection_co_change LIMIT 1)",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if !has_rows {
        super::run_with_immediate_tx(conn, |tx| {
            set_projection_co_change_pruned_on_open_tx(tx)?;
            Ok(())
        })?;
        return Ok(0);
    }

    super::run_with_immediate_tx(conn, |tx| {
        let deleted_rows = prune_projection_co_change_tx(tx)?;
        set_projection_co_change_pruned_on_open_tx(tx)?;
        Ok(deleted_rows)
    })
}

pub(super) fn apply_projection_co_change_deltas_tx(
    tx: &Transaction<'_>,
    deltas: &[prism_projections::CoChangeDelta],
) -> Result<()> {
    if deltas.is_empty() {
        return Ok(());
    }
    let mut aggregated =
        HashMap::<(prism_ir::LineageId, prism_ir::LineageId), u64>::with_capacity(deltas.len());
    let mut touched_sources = BTreeSet::<prism_ir::LineageId>::new();
    for delta in deltas {
        touched_sources.insert(delta.source_lineage.clone());
        *aggregated
            .entry((delta.source_lineage.clone(), delta.target_lineage.clone()))
            .or_insert(0) += u64::from(delta.count_delta);
    }

    let mut stmt = tx.prepare_cached(
        "INSERT INTO projection_co_change(source_lineage, target_lineage, count)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(source_lineage, target_lineage)
         DO UPDATE SET count = projection_co_change.count + excluded.count",
    )?;
    for ((source_lineage, target_lineage), count_delta) in aggregated {
        stmt.execute(params![
            source_lineage.0.as_str(),
            target_lineage.0.as_str(),
            i64::try_from(count_delta)?
        ])?;
    }
    if !touched_sources.is_empty() {
        prune_projection_co_change_sources_tx(tx, touched_sources.iter())?;
    }
    set_projection_materialization_flag_tx(tx, PROJECTION_HAS_CO_CHANGE_KEY, true)?;
    Ok(())
}

fn prune_projection_co_change_sources_tx<'a, I>(tx: &Transaction<'_>, sources: I) -> Result<usize>
where
    I: IntoIterator<Item = &'a prism_ir::LineageId>,
{
    let mut deleted_rows = 0;
    let mut stmt = tx.prepare_cached(
        "DELETE FROM projection_co_change
         WHERE source_lineage = ?1
           AND target_lineage IN (
               SELECT target_lineage
               FROM projection_co_change
               WHERE source_lineage = ?1
               ORDER BY count DESC, target_lineage
               LIMIT -1 OFFSET ?2
           )",
    )?;
    for source_lineage in sources {
        deleted_rows += stmt.execute(params![
            source_lineage.0.as_str(),
            MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE as i64
        ])?;
    }
    Ok(deleted_rows)
}

fn prune_projection_co_change_tx(tx: &Transaction<'_>) -> Result<usize> {
    let deleted_rows = tx.execute(
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
    Ok(deleted_rows)
}

fn set_projection_co_change_pruned_on_open_tx(tx: &Transaction<'_>) -> Result<()> {
    tx.execute(
        "INSERT INTO metadata(key, value) VALUES (?1, 1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![PROJECTION_CO_CHANGE_PRUNED_ON_OPEN_KEY],
    )?;
    Ok(())
}

pub(super) fn apply_projection_validation_deltas_tx(
    tx: &Transaction<'_>,
    deltas: &[prism_projections::ValidationDelta],
) -> Result<()> {
    if deltas.is_empty() {
        return Ok(());
    }
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
    set_projection_materialization_flag_tx(tx, PROJECTION_HAS_VALIDATION_KEY, true)?;
    Ok(())
}

fn metadata_or_row_presence(conn: &Connection, key: &str, exists_sql: &str) -> Result<bool> {
    let persisted = super::metadata_value(conn, key)?;
    if persisted > 0 {
        return Ok(true);
    }
    Ok(conn.query_row(exists_sql, [], |row| row.get::<_, bool>(0))?)
}

fn set_projection_materialization_metadata_tx(
    tx: &Transaction<'_>,
    metadata: crate::ProjectionMaterializationMetadata,
) -> Result<()> {
    set_projection_materialization_flag_tx(
        tx,
        PROJECTION_HAS_CO_CHANGE_KEY,
        metadata.has_co_change,
    )?;
    set_projection_materialization_flag_tx(
        tx,
        PROJECTION_HAS_VALIDATION_KEY,
        metadata.has_validation,
    )?;
    set_projection_materialization_flag_tx(
        tx,
        PROJECTION_HAS_KNOWLEDGE_KEY,
        metadata.has_knowledge,
    )?;
    Ok(())
}

fn set_projection_materialization_flag_tx(
    tx: &Transaction<'_>,
    key: &str,
    value: bool,
) -> Result<()> {
    tx.execute(
        "INSERT INTO metadata(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, if value { 1 } else { 0 }],
    )?;
    Ok(())
}

fn update_projection_knowledge_flag_tx(tx: &Transaction<'_>) -> Result<()> {
    let has_concepts = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM projection_curated_concept LIMIT 1)",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    let has_relations = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM projection_concept_relation LIMIT 1)",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    set_projection_materialization_flag_tx(
        tx,
        PROJECTION_HAS_KNOWLEDGE_KEY,
        has_concepts || has_relations,
    )
}

fn normalized_co_change_rows(
    snapshot: &prism_projections::ProjectionSnapshot,
) -> Vec<(prism_ir::LineageId, prism_projections::CoChangeRecord)> {
    let mut rows = Vec::new();
    for (source, neighbors) in &snapshot.co_change_by_lineage {
        let mut neighbors = neighbors.clone();
        neighbors.sort_by(|left, right| {
            right
                .count
                .cmp(&left.count)
                .then_with(|| left.lineage.0.cmp(&right.lineage.0))
        });
        neighbors.truncate(MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE);
        rows.extend(neighbors.into_iter().map(|record| (source.clone(), record)));
    }
    rows
}
