use std::thread;
use std::time::Duration;

use anyhow::Result;
use rusqlite::ffi::ErrorCode;

const SQLITE_READ_RETRY_ATTEMPTS: usize = 10;
const SQLITE_READ_RETRY_DELAY: Duration = Duration::from_millis(20);

pub(super) fn retry_on_transient_sqlite_read<T>(mut op: impl FnMut() -> Result<T>) -> Result<T> {
    let mut attempt = 0usize;
    loop {
        match op() {
            Ok(value) => return Ok(value),
            Err(error)
                if is_transient_sqlite_read_error(&error)
                    && attempt + 1 < SQLITE_READ_RETRY_ATTEMPTS =>
            {
                attempt += 1;
                thread::sleep(SQLITE_READ_RETRY_DELAY);
            }
            Err(error) => return Err(error),
        }
    }
}

fn is_transient_sqlite_read_error(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        if let Some(sqlite_error) = cause.downcast_ref::<rusqlite::Error>() {
            if matches!(
                sqlite_error,
                rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error {
                        code: ErrorCode::SchemaChanged
                            | ErrorCode::DatabaseBusy
                            | ErrorCode::DatabaseLocked,
                        ..
                    },
                    _
                )
            ) {
                return true;
            }
        }

        let text = cause.to_string().to_ascii_lowercase();
        text.contains("database schema has changed") || text.contains("no such table:")
    })
}

#[cfg(test)]
mod tests {
    use anyhow::{anyhow, Result};
    use rusqlite::ffi::ErrorCode;

    use super::retry_on_transient_sqlite_read;

    #[test]
    fn retries_schema_changed_once_then_succeeds() {
        let mut attempts = 0usize;
        let value = retry_on_transient_sqlite_read(|| -> Result<&'static str> {
            attempts += 1;
            if attempts == 1 {
                return Err(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error {
                        code: ErrorCode::SchemaChanged,
                        extended_code: 17,
                    },
                    Some("database schema has changed".to_string()),
                )
                .into());
            }
            Ok("ok")
        })
        .unwrap();

        assert_eq!(value, "ok");
        assert_eq!(attempts, 2);
    }

    #[test]
    fn retries_no_such_table_once_then_succeeds() {
        let mut attempts = 0usize;
        let value = retry_on_transient_sqlite_read(|| -> Result<&'static str> {
            attempts += 1;
            if attempts == 1 {
                return Err(anyhow!(
                    "no such table: shared_runtime.history_node_lineages"
                ));
            }
            Ok("ok")
        })
        .unwrap();

        assert_eq!(value, "ok");
        assert_eq!(attempts, 2);
    }

    #[test]
    fn does_not_retry_unrelated_errors() {
        let mut attempts = 0usize;
        let error = retry_on_transient_sqlite_read(|| -> Result<()> {
            attempts += 1;
            Err(anyhow!("malformed snapshot payload"))
        })
        .unwrap_err();

        assert_eq!(attempts, 1);
        assert!(error.to_string().contains("malformed snapshot payload"));
    }
}
