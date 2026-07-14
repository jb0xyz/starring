use std::fs;
use std::path::Path;

use design_harness::{SessionSnapshot, DEFAULT_SYSTEM_PROMPT, SESSION_SNAPSHOT_VERSION};
use rusqlite::{params, Connection, OptionalExtension};
use thiserror::Error;

const STORE_SCHEMA_VERSION: u32 = 2;
const LATEST_MIGRATABLE_SNAPSHOT_VERSION: u32 = 5;

#[derive(Clone, Debug, PartialEq)]
pub struct LoadedSession {
    pub generation: u64,
    pub snapshot: SessionSnapshot,
}

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

    fn initialize(mut connection: Connection) -> Result<Self, StoreError> {
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS harness_metadata (
                 key TEXT PRIMARY KEY NOT NULL,
                 value TEXT NOT NULL
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
                match found {
                    1 => migrate_store_v1_to_v2(&mut connection)?,
                    STORE_SCHEMA_VERSION => {}
                    _ => {
                        return Err(StoreError::UnsupportedSchemaVersion {
                            expected: STORE_SCHEMA_VERSION,
                            found,
                        });
                    }
                }
            }
            None => {
                let transaction = connection.transaction()?;
                transaction.execute_batch(
                    "CREATE TABLE IF NOT EXISTS harness_sessions (
                         session_id TEXT PRIMARY KEY NOT NULL,
                         snapshot_json TEXT NOT NULL,
                         updated_at INTEGER NOT NULL,
                         generation INTEGER NOT NULL DEFAULT 1
                     );",
                )?;
                if !has_generation_column(&transaction)? {
                    transaction.execute(
                        "ALTER TABLE harness_sessions
                         ADD COLUMN generation INTEGER NOT NULL DEFAULT 1",
                        [],
                    )?;
                }
                transaction.execute(
                    "INSERT INTO harness_metadata (key, value) VALUES ('schema_version', ?1)",
                    [STORE_SCHEMA_VERSION.to_string()],
                )?;
                transaction.commit()?;
            }
        }
        Ok(Self { connection })
    }

    pub fn load(&self, session_id: &str) -> Result<Option<SessionSnapshot>, StoreError> {
        Ok(self
            .load_versioned(session_id)?
            .map(|loaded| loaded.snapshot))
    }

    pub fn load_versioned(&self, session_id: &str) -> Result<Option<LoadedSession>, StoreError> {
        let stored = self
            .connection
            .query_row(
                "SELECT generation, snapshot_json
                 FROM harness_sessions
                 WHERE session_id = ?1",
                [session_id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let Some((generation, snapshot_json)) = stored else {
            return Ok(None);
        };
        let generation = parse_generation(session_id, generation)?;
        let snapshot = decode_snapshot(session_id, &snapshot_json)?;
        Ok(Some(LoadedSession {
            generation,
            snapshot,
        }))
    }

    pub fn save(&mut self, session_id: &str, snapshot: &SessionSnapshot) -> Result<(), StoreError> {
        let expected_generation = self
            .load_versioned(session_id)?
            .map_or(0, |loaded| loaded.generation);
        self.save_compare_and_swap(session_id, expected_generation, snapshot)?;
        Ok(())
    }

    pub fn save_compare_and_swap(
        &mut self,
        session_id: &str,
        expected_generation: u64,
        snapshot: &SessionSnapshot,
    ) -> Result<u64, StoreError> {
        if snapshot.schema_version != SESSION_SNAPSHOT_VERSION {
            return Err(StoreError::UnsupportedSnapshotVersion {
                expected: SESSION_SNAPSHOT_VERSION,
                found: snapshot.schema_version,
            });
        }
        let snapshot_json = serde_json::to_string(snapshot)?;
        let transaction = self.connection.transaction()?;
        let next_generation = if expected_generation == 0 {
            let affected = transaction.execute(
                "INSERT INTO harness_sessions (
                     session_id,
                     snapshot_json,
                     updated_at,
                     generation
                 )
                 VALUES (?1, ?2, unixepoch(), 1)
                 ON CONFLICT(session_id) DO NOTHING",
                params![session_id, snapshot_json],
            )?;
            if affected == 1 {
                1
            } else {
                return Err(generation_conflict(
                    &transaction,
                    session_id,
                    expected_generation,
                )?);
            }
        } else {
            let next_generation = expected_generation.checked_add(1).ok_or_else(|| {
                StoreError::GenerationOverflow {
                    session_id: session_id.to_string(),
                    generation: expected_generation,
                }
            })?;
            let expected_generation_sql = generation_to_sql(session_id, expected_generation)?;
            let next_generation_sql = generation_to_sql(session_id, next_generation)?;
            let affected = transaction.execute(
                "UPDATE harness_sessions
                 SET snapshot_json = ?2,
                     updated_at = unixepoch(),
                     generation = ?3
                 WHERE session_id = ?1 AND generation = ?4",
                params![
                    session_id,
                    snapshot_json,
                    next_generation_sql,
                    expected_generation_sql
                ],
            )?;
            if affected == 1 {
                next_generation
            } else {
                return Err(generation_conflict(
                    &transaction,
                    session_id,
                    expected_generation,
                )?);
            }
        };
        transaction.commit()?;
        Ok(next_generation)
    }
}

fn migrate_store_v1_to_v2(connection: &mut Connection) -> Result<(), StoreError> {
    let transaction = connection.transaction()?;
    transaction.execute(
        "ALTER TABLE harness_sessions
         ADD COLUMN generation INTEGER NOT NULL DEFAULT 1",
        [],
    )?;
    transaction.execute(
        "UPDATE harness_metadata SET value = ?1 WHERE key = 'schema_version'",
        [STORE_SCHEMA_VERSION.to_string()],
    )?;
    transaction.commit()?;
    Ok(())
}

fn has_generation_column(transaction: &rusqlite::Transaction<'_>) -> Result<bool, StoreError> {
    Ok(transaction.query_row(
        "SELECT EXISTS(
             SELECT 1
             FROM pragma_table_info('harness_sessions')
             WHERE name = 'generation'
         )",
        [],
        |row| row.get::<_, bool>(0),
    )?)
}

fn decode_snapshot(session_id: &str, snapshot_json: &str) -> Result<SessionSnapshot, StoreError> {
    let mut value = serde_json::from_str::<serde_json::Value>(snapshot_json).map_err(|source| {
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
    if found != SESSION_SNAPSHOT_VERSION {
        if (2..=LATEST_MIGRATABLE_SNAPSHOT_VERSION).contains(&found) {
            migrate_snapshot(&mut value, found);
        } else {
            return Err(StoreError::UnsupportedSnapshotVersion {
                expected: SESSION_SNAPSHOT_VERSION,
                found,
            });
        }
    }
    let snapshot = serde_json::from_value::<SessionSnapshot>(value).map_err(|source| {
        StoreError::CorruptSnapshot {
            session_id: session_id.to_string(),
            source,
        }
    })?;
    Ok(snapshot)
}

fn parse_generation(session_id: &str, generation: i64) -> Result<u64, StoreError> {
    let generation = u64::try_from(generation).map_err(|_| StoreError::CorruptGeneration {
        session_id: session_id.to_string(),
        value: generation,
    })?;
    if generation == 0 {
        return Err(StoreError::CorruptGeneration {
            session_id: session_id.to_string(),
            value: 0,
        });
    }
    Ok(generation)
}

fn generation_to_sql(session_id: &str, generation: u64) -> Result<i64, StoreError> {
    i64::try_from(generation).map_err(|_| StoreError::GenerationOverflow {
        session_id: session_id.to_string(),
        generation,
    })
}

fn generation_conflict(
    transaction: &rusqlite::Transaction<'_>,
    session_id: &str,
    expected_generation: u64,
) -> Result<StoreError, StoreError> {
    let actual = transaction
        .query_row(
            "SELECT generation FROM harness_sessions WHERE session_id = ?1",
            [session_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .map(|generation| parse_generation(session_id, generation))
        .transpose()?;
    Ok(StoreError::GenerationConflict {
        session_id: session_id.to_string(),
        expected_generation,
        actual_generation: actual,
    })
}

fn migrate_snapshot(value: &mut serde_json::Value, found: u32) {
    let Some(root) = value.as_object_mut() else {
        return;
    };
    root.insert(
        "schema_version".to_string(),
        serde_json::Value::from(SESSION_SNAPSHOT_VERSION),
    );
    root.entry("intent_recipe".to_string())
        .or_insert(serde_json::Value::Null);
    if found >= 4 {
        return;
    }
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
    #[error(
        "session {session_id} generation conflict: expected {expected_generation}, actual {actual_generation:?}"
    )]
    GenerationConflict {
        session_id: String,
        expected_generation: u64,
        actual_generation: Option<u64>,
    },
    #[error("session {session_id} contains corrupt generation {value}")]
    CorruptGeneration { session_id: String, value: i64 },
    #[error("session {session_id} generation {generation} cannot be incremented or stored")]
    GenerationOverflow { session_id: String, generation: u64 },
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
    use rusqlite::Connection;

    use super::{SessionStore, StoreError, STORE_SCHEMA_VERSION};

    fn temporary_database(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "starring-design-harness-{label}-{}-{unique}.sqlite3",
            std::process::id()
        ))
    }

    #[test]
    fn in_memory_store_roundtrips_snapshot() {
        let mut store = SessionStore::open_in_memory().unwrap();
        let snapshot = DesignSession::new(()).snapshot();

        assert_eq!(snapshot.schema_version, SESSION_SNAPSHOT_VERSION);
        store.save("study", &snapshot).unwrap();

        let loaded = store.load_versioned("study").unwrap().unwrap();
        assert_eq!(loaded.generation, 1);
        assert_eq!(loaded.snapshot, snapshot);
        assert_eq!(store.load("study").unwrap(), Some(snapshot.clone()));
        let mut updated = snapshot;
        updated.prose_nudged = true;
        store.save("study", &updated).unwrap();
        assert_eq!(
            store.load_versioned("study").unwrap().unwrap().generation,
            2
        );
        assert_eq!(store.load("study").unwrap(), Some(updated));
        let planned = DesignSession::with_planned_config((), SessionConfig::default()).snapshot();
        store.save("planned", &planned).unwrap();
        assert_eq!(store.load("planned").unwrap(), Some(planned));
        assert_eq!(store.load("missing").unwrap(), None);
    }

    #[test]
    fn file_store_survives_close_and_reopen() {
        let path = temporary_database("reopen");
        let mut snapshot = DesignSession::new(()).snapshot();
        {
            let mut store = SessionStore::open(&path).unwrap();
            assert_eq!(
                store.save_compare_and_swap("study", 0, &snapshot).unwrap(),
                1
            );
            snapshot.prose_nudged = true;
            assert_eq!(
                store.save_compare_and_swap("study", 1, &snapshot).unwrap(),
                2
            );
        }
        let reopened = SessionStore::open(&path).unwrap();
        let loaded = reopened.load_versioned("study").unwrap().unwrap();
        assert_eq!(loaded.generation, 2);
        assert_eq!(loaded.snapshot, snapshot);
        drop(reopened);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn schema_one_migrates_rows_generations_and_metadata_in_place() {
        let path = temporary_database("migration");
        let snapshot = DesignSession::new(()).snapshot();
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE harness_metadata (
                         key TEXT PRIMARY KEY NOT NULL,
                         value TEXT NOT NULL
                     );
                     CREATE TABLE harness_sessions (
                         session_id TEXT PRIMARY KEY NOT NULL,
                         snapshot_json TEXT NOT NULL,
                         updated_at INTEGER NOT NULL
                     );
                     INSERT INTO harness_metadata (key, value)
                     VALUES ('schema_version', '1'), ('retained', 'yes');",
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO harness_sessions (session_id, snapshot_json, updated_at)
                     VALUES (?1, ?2, 17)",
                    ["study", serde_json::to_string(&snapshot).unwrap().as_str()],
                )
                .unwrap();
        }

        let mut store = SessionStore::open(&path).unwrap();
        let loaded = store.load_versioned("study").unwrap().unwrap();
        assert_eq!(loaded.generation, 1);
        assert_eq!(loaded.snapshot, snapshot);
        assert_eq!(
            store
                .connection
                .query_row(
                    "SELECT value FROM harness_metadata WHERE key = 'schema_version'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            STORE_SCHEMA_VERSION.to_string()
        );
        assert_eq!(
            store
                .connection
                .query_row(
                    "SELECT value FROM harness_metadata WHERE key = 'retained'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "yes"
        );
        assert_eq!(
            store
                .connection
                .query_row(
                    "SELECT updated_at FROM harness_sessions WHERE session_id = 'study'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            17
        );
        assert_eq!(
            store
                .save_compare_and_swap("study", loaded.generation, &snapshot)
                .unwrap(),
            2
        );
        drop(store);
        let reopened = SessionStore::open(&path).unwrap();
        assert_eq!(
            reopened
                .load_versioned("study")
                .unwrap()
                .unwrap()
                .generation,
            2
        );
        drop(reopened);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn missing_metadata_recovers_a_legacy_session_table_atomically() {
        let path = temporary_database("metadata-recovery");
        let snapshot = DesignSession::new(()).snapshot();
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE harness_sessions (
                         session_id TEXT PRIMARY KEY NOT NULL,
                         snapshot_json TEXT NOT NULL,
                         updated_at INTEGER NOT NULL
                     );",
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO harness_sessions (session_id, snapshot_json, updated_at)
                     VALUES (?1, ?2, 29)",
                    [
                        "recovered",
                        serde_json::to_string(&snapshot).unwrap().as_str(),
                    ],
                )
                .unwrap();
        }

        let store = SessionStore::open(&path).unwrap();
        let loaded = store.load_versioned("recovered").unwrap().unwrap();
        assert_eq!(loaded.generation, 1);
        assert_eq!(loaded.snapshot, snapshot);
        assert_eq!(
            store
                .connection
                .query_row(
                    "SELECT value FROM harness_metadata WHERE key = 'schema_version'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            STORE_SCHEMA_VERSION.to_string()
        );
        assert_eq!(
            store
                .connection
                .query_row(
                    "SELECT updated_at FROM harness_sessions WHERE session_id = 'recovered'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            29
        );
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn independent_writers_allow_exactly_one_generation_winner() {
        let path = temporary_database("cas-race");
        let snapshot = DesignSession::new(()).snapshot();
        {
            let mut store = SessionStore::open(&path).unwrap();
            assert_eq!(
                store.save_compare_and_swap("study", 0, &snapshot).unwrap(),
                1
            );
        }
        let mut first = SessionStore::open(&path).unwrap();
        let mut second = SessionStore::open(&path).unwrap();
        let first_loaded = first.load_versioned("study").unwrap().unwrap();
        let second_loaded = second.load_versioned("study").unwrap().unwrap();
        assert_eq!(first_loaded.generation, second_loaded.generation);
        let mut first_snapshot = first_loaded.snapshot;
        first_snapshot.prose_nudged = true;
        let mut stale_snapshot = second_loaded.snapshot;
        stale_snapshot.adaptive_enabled = false;

        assert_eq!(
            first
                .save_compare_and_swap("study", first_loaded.generation, &first_snapshot)
                .unwrap(),
            2
        );
        assert!(matches!(
            second.save_compare_and_swap("study", second_loaded.generation, &stale_snapshot),
            Err(StoreError::GenerationConflict {
                expected_generation: 1,
                actual_generation: Some(2),
                ..
            })
        ));
        let final_value = second.load_versioned("study").unwrap().unwrap();
        assert_eq!(final_value.generation, 2);
        assert_eq!(final_value.snapshot, first_snapshot);
        assert_ne!(final_value.snapshot, stale_snapshot);

        drop(first);
        drop(second);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn generation_conflicts_report_presence_and_absence_without_writing() {
        let mut store = SessionStore::open_in_memory().unwrap();
        let snapshot = DesignSession::new(()).snapshot();
        assert!(matches!(
            store.save_compare_and_swap("missing", 4, &snapshot),
            Err(StoreError::GenerationConflict {
                expected_generation: 4,
                actual_generation: None,
                ..
            })
        ));
        assert!(store.load_versioned("missing").unwrap().is_none());
        assert_eq!(
            store.save_compare_and_swap("study", 0, &snapshot).unwrap(),
            1
        );
        assert!(matches!(
            store.save_compare_and_swap("study", 0, &snapshot),
            Err(StoreError::GenerationConflict {
                expected_generation: 0,
                actual_generation: Some(1),
                ..
            })
        ));
        assert_eq!(
            store.load_versioned("study").unwrap().unwrap().generation,
            1
        );
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
        store
            .connection
            .execute(
                "INSERT INTO harness_sessions (session_id, snapshot_json, updated_at) VALUES (?1, ?2, 0)",
                ["future-load", serde_json::to_string(&snapshot).unwrap().as_str()],
            )
            .unwrap();
        assert!(matches!(
            store.load("future-load"),
            Err(StoreError::UnsupportedSnapshotVersion { expected, found })
                if expected == SESSION_SNAPSHOT_VERSION
                    && found == SESSION_SNAPSHOT_VERSION + 1
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
    fn version_four_snapshots_migrate_without_rewriting_session_mode_content() {
        for planned in [false, true] {
            let store = SessionStore::open_in_memory().unwrap();
            let session = if planned {
                DesignSession::with_planned_config((), SessionConfig::default())
            } else {
                DesignSession::new(())
            };
            let snapshot = session.snapshot();
            let expected_prompt = snapshot.messages[0].content.clone();
            let expected_adaptive = snapshot.adaptive_enabled;
            let mut value = serde_json::to_value(snapshot).unwrap();
            value["schema_version"] = serde_json::json!(4);
            let id = if planned { "v4-planned" } else { "v4-default" };
            store
                .connection
                .execute(
                    "INSERT INTO harness_sessions (session_id, snapshot_json, updated_at) VALUES (?1, ?2, 0)",
                    [id, value.to_string().as_str()],
                )
                .unwrap();

            let migrated = store.load(id).unwrap().unwrap();

            assert_eq!(migrated.schema_version, SESSION_SNAPSHOT_VERSION);
            assert_eq!(migrated.messages[0].content, expected_prompt);
            assert_eq!(migrated.adaptive_enabled, expected_adaptive);
            if planned {
                assert!(
                    DesignSession::restore_planned((), SessionConfig::default(), migrated)
                        .unwrap()
                        .planned_enabled()
                );
            } else {
                assert!(DesignSession::restore((), SessionConfig::default(), migrated).is_ok());
            }
        }
    }

    #[test]
    fn version_five_snapshots_default_the_intent_recipe_state() {
        let store = SessionStore::open_in_memory().unwrap();
        let mut value = serde_json::to_value(DesignSession::new(()).snapshot()).unwrap();
        value["schema_version"] = serde_json::json!(5);
        value.as_object_mut().unwrap().remove("intent_recipe");
        let observability = value["observability"].as_object_mut().unwrap();
        for field in [
            "intent_route_calls",
            "intent_proposal_acceptances",
            "intent_resolution_acceptances",
            "intent_compile_attempts",
            "intent_compile_successes",
            "intent_commits",
            "intent_rollbacks",
            "intent_conflicts",
            "intent_stale_revision_rejections",
            "intent_extraction_failures",
            "intent_fallback_routes",
            "intent_compiled_operations",
        ] {
            observability.remove(field);
        }
        store
            .connection
            .execute(
                "INSERT INTO harness_sessions (session_id, snapshot_json, updated_at)
                 VALUES (?1, ?2, 0)",
                ["v5", value.to_string().as_str()],
            )
            .unwrap();

        let migrated = store.load("v5").unwrap().unwrap();
        let migrated_value = serde_json::to_value(&migrated).unwrap();
        assert_eq!(migrated.schema_version, SESSION_SNAPSHOT_VERSION);
        assert_eq!(migrated_value["intent_recipe"], serde_json::Value::Null);
        assert_eq!(migrated.observability.intent_route_calls, 0);
        assert!(migrated.observability.intent_fallback_routes.is_empty());
    }

    #[test]
    fn database_schema_version_mismatch_is_typed() {
        let path = temporary_database("schema");
        {
            let store = SessionStore::open(&path).unwrap();
            store
                .connection
                .execute(
                    "UPDATE harness_metadata SET value = '3' WHERE key = 'schema_version'",
                    [],
                )
                .unwrap();
        }
        assert!(matches!(
            SessionStore::open(&path),
            Err(StoreError::UnsupportedSchemaVersion {
                expected: 2,
                found: 3
            })
        ));
        std::fs::remove_file(path).unwrap();
    }
}
