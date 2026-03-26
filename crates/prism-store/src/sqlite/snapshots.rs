use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

pub(super) fn load_snapshot_row<T>(conn: &Connection, key: &str) -> Result<Option<T>>
where
    T: for<'de> Deserialize<'de>,
{
    let raw = conn
        .query_row(
            "SELECT value FROM snapshots WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    raw.map(|value| serde_json::from_str(&value))
        .transpose()
        .map_err(Into::into)
}

pub(super) fn save_snapshot_row<T>(conn: &Connection, key: &str, snapshot: &T) -> Result<()>
where
    T: Serialize,
{
    conn.execute(
        "INSERT INTO snapshots(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, serde_json::to_string(snapshot)?],
    )?;
    Ok(())
}

pub(super) fn save_snapshot_row_tx<T>(tx: &Transaction<'_>, key: &str, snapshot: &T) -> Result<()>
where
    T: Serialize,
{
    tx.execute(
        "INSERT INTO snapshots(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, serde_json::to_string(snapshot)?],
    )?;
    Ok(())
}
