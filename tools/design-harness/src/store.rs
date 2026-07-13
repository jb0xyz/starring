use std::fs;
use std::path::Path;

use design_harness::{SessionSnapshot, DEFAULT_SYSTEM_PROMPT, SESSION_SNAPSHOT_VERSION};
use rusqlite::{params, Connection, OptionalExtension};
use thiserror::Error;

const STORE_SCHEMA_VERSION: u32 = 1;

pub struct SessionStore {
    connection: Connection,
}

impl SessionStore {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let connection = Connection::open(path)?;
        Self::initialize(connection)
    }

    #[cfg(test)]
    fn open_in_memory() -> Result<Self, StoreError> {
        Self::initialize(Connection::open_in_memory()?)
    }

    fn initialize(connection: Connection) -> Result<Self, StoreError> {
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS harness_metadata (
                 key TEXT PRIMARY KEY NOT NULL,
                 value TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS harness_sessions (
                 session_id TEXT PRIMARY KEY NOT NULL,
                 snapshot_json TEXT NOT NULL,
                 updated_at INTEGER NOT NULL
             );",
        )?;
        let current = connection
            .query_row(
                "SELECT value FROM harness_metadata WHERE key = 'schema_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        match current {
            Some(value) => {
                let found = value
                    .parse::<u32>()
                    .map_err(|_| StoreError::CorruptSchemaVersion { value })?;
                if found != STORE_SCHEMA_VERSION {
                    return Err(StoreError::UnsupportedSchemaVersion {
                        expected: STORE_SCHEMA_VERSION,
                        found,
                    });
                }
            }
            None => {
                connection.execute(
                    "INSERT INTO harness_metadata (key, value) VALUES ('schema_version', ?1)",
                    [STORE_SCHEMA_VERSION.to_string()],
                )?;
            }
        }
        Ok(Self { connection })
    }

    pub fn load(&self, session_id: &str) -> Result<Option<SessionSnapshot>, StoreError> {
        let snapshot_json = self
            .connection
            .query_row(
                "SELECT snapshot_json FROM harness_sessions WHERE session_id = ?1",
                [session_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(snapshot_json) = snapshot_json else {
            return Ok(None);
        };
        let mut value =
            serde_json::from_str::<serde_json::Value>(&snapshot_json).map_err(|source| {
                StoreError::CorruptSnapshot {
                    session_id: session_id.to_string(),
                    source,
                }
            })?;
        let found = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .and_then(|version| u32::try_from(version).ok())
            .ok_or_else(|| StoreError::CorruptSnapshotSchemaVersion {
                session_id: session_id.to_string(),
            })?;
        if matches!(found, 2 | 3) {
            migrate_snapshot(&mut value, found);
        } else if found != SESSION_SNAPSHOT_VERSION {
            return Err(StoreError::UnsupportedSnapshotVersion {
                expected: SESSION_SNAPSHOT_VERSION,
                found,
            });
        }
        let snapshot = serde_json::from_value::<SessionSnapshot>(value).map_err(|source| {
            StoreError::CorruptSnapshot {
                session_id: session_id.to_string(),
                source,
            }
        })?;
        Ok(Some(snapshot))
    }

    pub fn save(&mut self, session_id: &str, snapshot: &SessionSnapshot) -> Result<(), StoreError> {
        if snapshot.schema_version != SESSION_SNAPSHOT_VERSION {
            return Err(StoreError::UnsupportedSnapshotVersion {
                expected: SESSION_SNAPSHOT_VERSION,
                found: snapshot.schema_version,
            });
        }
        let snapshot_json = serde_json::to_string(snapshot)?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO harness_sessions (session_id, snapshot_json, updated_at)
             VALUES (?1, ?2, unixepoch())
             ON CONFLICT(session_id) DO UPDATE SET
                 snapshot_json = excluded.snapshot_json,
                 updated_at = excluded.updated_at",
            params![session_id, snapshot_json],
        )?;
        transaction.commit()?;
        Ok(())
    }
}

fn migrate_snapshot(value: &mut serde_json::Value, found: u32) {
    let Some(root) = value.as_object_mut() else {
        return;
    };
    root.insert(
        "schema_version".to_string(),
        serde_json::Value::from(SESSION_SNAPSHOT_VERSION),
    );
    root.entry("turn_state".to_string())
        .or_insert(serde_json::Value::Null);
    root.entry("adaptive_turn".to_string())
        .or_insert(serde_json::Value::Null);
    root.insert(
        "adaptive_enabled".to_string(),
        serde_json::Value::Bool(true),
    );
    root.entry("brief_history".to_string())
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
    if let Some(system) = root
        .get_mut("messages")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|messages| messages.first_mut())
        .and_then(serde_json::Value::as_object_mut)
    {
        system.insert(
            "content".to_string(),
            serde_json::Value::String(DEFAULT_SYSTEM_PROMPT.to_string()),
        );
    }
    if found == 2 {
        root.insert("turn_state".to_string(), serde_json::Value::Null);
    }
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("failed to access the session database filesystem: {0}")]
    Filesystem(#[from] std::io::Error),
    #[error("session database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("session snapshot serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("session database schema version is corrupt: {value}")]
    CorruptSchemaVersion { value: String },
    #[error("unsupported session database schema version {found}; expected {expected}")]
    UnsupportedSchemaVersion { expected: u32, found: u32 },
    #[error("session {session_id} contains a corrupt snapshot: {source}")]
    CorruptSnapshot {
        session_id: String,
        source: serde_json::Error,
    },
    #[error("session {session_id} contains a corrupt snapshot schema version")]
    CorruptSnapshotSchemaVersion { session_id: String },
    #[error("unsupported session snapshot version {found}; expected {expected}")]
    UnsupportedSnapshotVersion { expected: u32, found: u32 },
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use design_harness::{
        DesignSession, SessionConfig, DEFAULT_SYSTEM_PROMPT, SESSION_SNAPSHOT_VERSION,
    };

    use super::{SessionStore, StoreError};

    #[test]
    fn in_memory_store_roundtrips_snapshot() {
        let mut store = SessionStore::open_in_memory().unwrap();
        let snapshot = DesignSession::new(()).snapshot();

        store.save("study", &snapshot).unwrap();

        assert_eq!(store.load("study").unwrap(), Some(snapshot.clone()));
        let mut updated = snapshot;
        updated.prose_nudged = true;
        store.save("study", &updated).unwrap();
        assert_eq!(store.load("study").unwrap(), Some(updated));
        assert_eq!(store.load("missing").unwrap(), None);
    }

    #[test]
    fn file_store_survives_close_and_reopen() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "starring-design-harness-{}-{unique}.sqlite3",
            std::process::id()
        ));
        let snapshot = DesignSession::new(()).snapshot();
        {
            let mut store = SessionStore::open(&path).unwrap();
            store.save("study", &snapshot).unwrap();
        }
        let reopened = SessionStore::open(&path).unwrap();
        assert_eq!(reopened.load("study").unwrap(), Some(snapshot));
        drop(reopened);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn corrupt_snapshot_and_versions_are_typed() {
        let store = SessionStore::open_in_memory().unwrap();
        store
            .connection
            .execute(
                "INSERT INTO harness_sessions (session_id, snapshot_json, updated_at) VALUES (?1, ?2, 0)",
                ["broken", "not-json"],
            )
            .unwrap();
        assert!(matches!(
            store.load("broken"),
            Err(StoreError::CorruptSnapshot { .. })
        ));

        let mut snapshot = DesignSession::new(()).snapshot();
        snapshot.schema_version += 1;
        let mut store = store;
        assert!(matches!(
            store.save("future", &snapshot),
            Err(StoreError::UnsupportedSnapshotVersion { .. })
        ));
    }

    #[test]
    fn legacy_snapshot_version_is_rejected_before_current_shape_decode() {
        let store = SessionStore::open_in_memory().unwrap();
        let snapshot = DesignSession::new(()).snapshot();
        let mut legacy = serde_json::to_value(snapshot).unwrap();
        legacy["schema_version"] = serde_json::json!(1);
        legacy.as_object_mut().unwrap().remove("repair_state");
        let observability = legacy["observability"].as_object_mut().unwrap();
        for field in [
            "repair_attempts",
            "repair_successes",
            "repair_failures",
            "repair_escalations",
        ] {
            observability.remove(field);
        }
        store
            .connection
            .execute(
                "INSERT INTO harness_sessions (session_id, snapshot_json, updated_at) VALUES (?1, ?2, 0)",
                ["legacy", legacy.to_string().as_str()],
            )
            .unwrap();

        assert!(matches!(
            store.load("legacy"),
            Err(StoreError::UnsupportedSnapshotVersion {
                expected: SESSION_SNAPSHOT_VERSION,
                found: 1
            })
        ));
    }

    #[test]
    fn version_two_and_three_snapshots_migrate_to_adaptive_sessions() {
        for version in [2, 3] {
            let store = SessionStore::open_in_memory().unwrap();
            let mut value = serde_json::to_value(DesignSession::new(()).snapshot()).unwrap();
            let root = value.as_object_mut().unwrap();
            root.insert("schema_version".to_string(), version.into());
            root.remove("adaptive_turn");
            root.remove("adaptive_enabled");
            root.remove("brief_history");
            if version == 2 {
                root.remove("turn_state");
            }
            root.get_mut("messages")
                .and_then(serde_json::Value::as_array_mut)
                .map(|messages| {
                    messages
                        .first_mut()
                        .and_then(serde_json::Value::as_object_mut)
                        .unwrap()
                        .insert(
                            "content".to_string(),
                            serde_json::Value::String("legacy prompt".to_string()),
                        );
                    messages.push(serde_json::json!({
                        "role": "system",
                        "content": concat!(
                            "DRAFT_STATE:{\"revision\":0,\"validated_revision\":null,",
                            "\"simulated_revision\":null,\"panels\":[],\"modals\":[],",
                            "\"rules\":[],\"created_aliases\":{\"roles\":[],",
                            "\"channels\":[],\"messages\":[],\"instances\":[]},",
                            "\"unresolved_references\":[],\"failure_signatures\":{},",
                            "\"last_error\":null,\"repair_state\":null,",
                            "\"current_human_intent\":null,\"recent_human_intent\":[]}"
                        )
                    }));
                })
                .unwrap();
            let id = format!("v{version}");
            store
                .connection
                .execute(
                    "INSERT INTO harness_sessions (session_id, snapshot_json, updated_at) VALUES (?1, ?2, 0)",
                    [id.as_str(), value.to_string().as_str()],
                )
                .unwrap();

            let migrated = store.load(&id).unwrap().unwrap();

            assert_eq!(migrated.schema_version, SESSION_SNAPSHOT_VERSION);
            assert!(migrated.adaptive_enabled);
            assert!(migrated.adaptive_turn.is_none());
            assert!(migrated.brief_history.is_empty());
            assert_eq!(migrated.messages[0].content, DEFAULT_SYSTEM_PROMPT);
            assert_eq!(migrated.messages.len(), 2);
            assert!(DesignSession::restore((), SessionConfig::default(), migrated).is_ok());
        }
    }

    #[test]
    fn database_schema_version_mismatch_is_typed() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "starring-design-harness-schema-{}-{unique}.sqlite3",
            std::process::id()
        ));
        {
            let store = SessionStore::open(&path).unwrap();
            store
                .connection
                .execute(
                    "UPDATE harness_metadata SET value = '2' WHERE key = 'schema_version'",
                    [],
                )
                .unwrap();
        }
        assert!(matches!(
            SessionStore::open(&path),
            Err(StoreError::UnsupportedSchemaVersion {
                expected: 1,
                found: 2
            })
        ));
        std::fs::remove_file(path).unwrap();
    }
}
