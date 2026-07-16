use std::env;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;

use std::collections::BTreeSet;

use design_harness::{ResourceBindingMap, SessionConfig};
use desired_state::ResourceKey;
use discord_model::{ChannelId, RoleId};
use serde::Deserialize;

const DEFAULT_BASE_URL: &str = "https://llm-api.starring.co.kr/v1";
const DEFAULT_CODEX_WORKER_URL: &str = "http://127.0.0.1:18181";
pub const SERVING_AUTH_MODE: &str = "chatgpt";
pub const SERVING_MODEL: &str = "gpt-5.6-luna";
pub const SERVING_PROVIDER: &str = "codex_chatgpt";
pub const SERVING_REASONING_EFFORT: &str = "medium";
pub(crate) const LEGACY_SERVING_MODEL: &str = "gemma4:12b-mlx";
const DEFAULT_SESSION_ID: &str = "default";

pub struct EdgeConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub session_config: SessionConfig,
}

pub struct CodexWorkerConfig {
    pub base_url: String,
    pub token: String,
    pub session_config: SessionConfig,
}

pub struct PersistenceConfig {
    pub db_path: PathBuf,
    pub session_id: String,
    pub mode: HarnessMode,
    pub bindings: Option<ResourceBindingMap>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HarnessMode {
    Adaptive,
    TypedPlan,
    IntentRecipe,
}

impl PersistenceConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        persistence_config_from(|name| env::var(name).ok())
    }
}

impl EdgeConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        edge_config_from(|name| env::var(name).ok())
    }
}

impl CodexWorkerConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        codex_worker_config_from(|name| env::var(name).ok())
    }
}

pub fn intent_bindings_from_env() -> Result<ResourceBindingMap, ConfigError> {
    let document = env::var("STARRING_HARNESS_BINDINGS_JSON")
        .map_err(|_| ConfigError::MissingIntentBindings)?;
    parse_bindings(&document)
}

fn edge_config_from<F>(mut value: F) -> Result<EdgeConfig, ConfigError>
where
    F: FnMut(&str) -> Option<String>,
{
    let base_url = value("STARRING_LLM_BASE_URL").unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
    if base_url.trim().is_empty() {
        return Err(ConfigError::EmptyBaseUrl);
    }
    let api_key = value("STARRING_LLM_API_KEY").ok_or(ConfigError::MissingApiKey)?;
    if api_key.trim().is_empty() {
        return Err(ConfigError::MissingApiKey);
    }
    let model = value("STARRING_LLM_MODEL").unwrap_or_else(|| LEGACY_SERVING_MODEL.to_string());
    if model.trim().is_empty() {
        return Err(ConfigError::EmptyModel);
    }
    if model != LEGACY_SERVING_MODEL {
        return Err(ConfigError::UnsupportedModel);
    }
    let session_config = session_config_from(value)?;
    Ok(EdgeConfig {
        base_url,
        api_key,
        model,
        session_config,
    })
}

fn codex_worker_config_from<F>(mut value: F) -> Result<CodexWorkerConfig, ConfigError>
where
    F: FnMut(&str) -> Option<String>,
{
    let base_url =
        value("STARRING_CODEX_WORKER_URL").unwrap_or_else(|| DEFAULT_CODEX_WORKER_URL.to_string());
    if base_url.trim().is_empty() {
        return Err(ConfigError::EmptyCodexWorkerUrl);
    }
    let token = value("STARRING_CODEX_WORKER_TOKEN")
        .filter(|token| !token.trim().is_empty())
        .ok_or(ConfigError::MissingCodexWorkerToken)?;
    let session_config = session_config_from(value)?;
    Ok(CodexWorkerConfig {
        base_url,
        token,
        session_config,
    })
}

fn session_config_from<F>(mut value: F) -> Result<SessionConfig, ConfigError>
where
    F: FnMut(&str) -> Option<String>,
{
    let defaults = SessionConfig::default();
    Ok(SessionConfig {
        max_model_calls: parse_bound(
            value("STARRING_HARNESS_MAX_MODEL_CALLS"),
            "STARRING_HARNESS_MAX_MODEL_CALLS",
            defaults.max_model_calls,
        )?,
        max_tool_calls: parse_bound(
            value("STARRING_HARNESS_MAX_TOOL_CALLS"),
            "STARRING_HARNESS_MAX_TOOL_CALLS",
            defaults.max_tool_calls,
        )?,
        max_gate_failures: parse_bound(
            value("STARRING_HARNESS_MAX_GATE_FAILURES"),
            "STARRING_HARNESS_MAX_GATE_FAILURES",
            defaults.max_gate_failures,
        )?,
        context_char_budget: parse_bound(
            value("STARRING_HARNESS_CONTEXT_CHARS"),
            "STARRING_HARNESS_CONTEXT_CHARS",
            defaults.context_char_budget,
        )?,
    })
}

fn parse_bound(
    value: Option<String>,
    name: &'static str,
    default: usize,
) -> Result<usize, ConfigError> {
    let Some(value) = value else {
        return Ok(default);
    };
    value
        .parse::<usize>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or(ConfigError::InvalidBound { name })
}

fn persistence_config_from<F>(mut value: F) -> Result<PersistenceConfig, ConfigError>
where
    F: FnMut(&str) -> Option<String>,
{
    let session_id =
        value("STARRING_HARNESS_SESSION_ID").unwrap_or_else(|| DEFAULT_SESSION_ID.to_string());
    if session_id.trim().is_empty() {
        return Err(ConfigError::EmptySessionId);
    }
    let mode = parse_harness_mode(
        value("STARRING_HARNESS_MODE"),
        value("STARRING_HARNESS_PLANNED"),
    )?;
    let bindings = match (mode, value("STARRING_HARNESS_BINDINGS_JSON")) {
        (HarnessMode::IntentRecipe, Some(document)) => Some(parse_bindings(&document)?),
        (HarnessMode::IntentRecipe, None) => return Err(ConfigError::MissingIntentBindings),
        (_, Some(_)) => return Err(ConfigError::BindingsRequireIntentMode),
        (_, None) => None,
    };
    let db_path = match value("STARRING_HARNESS_DB_PATH") {
        Some(path) if !path.trim().is_empty() => PathBuf::from(path),
        Some(_) => return Err(ConfigError::EmptyDatabasePath),
        None => {
            let home = value("HOME")
                .filter(|home| !home.trim().is_empty())
                .ok_or(ConfigError::MissingHomeForDefaultDatabasePath)?;
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("starring")
                .join("design-harness.sqlite3")
        }
    };
    Ok(PersistenceConfig {
        db_path,
        session_id,
        mode,
        bindings,
    })
}

fn parse_harness_mode(
    mode: Option<String>,
    legacy_planned: Option<String>,
) -> Result<HarnessMode, ConfigError> {
    if mode.is_some() && legacy_planned.is_some() {
        return Err(ConfigError::ConflictingHarnessModes);
    }
    if let Some(mode) = mode {
        return match mode.as_str() {
            "adaptive" => Ok(HarnessMode::Adaptive),
            "typed_plan" => Ok(HarnessMode::TypedPlan),
            "intent_recipe" => Ok(HarnessMode::IntentRecipe),
            _ => Err(ConfigError::InvalidHarnessMode),
        };
    }
    match legacy_planned {
        None => Ok(HarnessMode::Adaptive),
        Some(value) if value == "0" || value == "false" => Ok(HarnessMode::Adaptive),
        Some(value) if value == "1" || value == "true" => Ok(HarnessMode::TypedPlan),
        Some(_) => Err(ConfigError::InvalidPlannedMode),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingDocumentV1 {
    schema_version: u32,
    #[serde(default)]
    channel_bindings: Vec<BindingEntryV1>,
    #[serde(default)]
    role_bindings: Vec<BindingEntryV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingEntryV1 {
    key: String,
    id: String,
}

fn parse_bindings(document: &str) -> Result<ResourceBindingMap, ConfigError> {
    if document.trim().is_empty() {
        return Err(ConfigError::InvalidBindingsJson);
    }
    let document: BindingDocumentV1 =
        serde_json::from_str(document).map_err(|_| ConfigError::InvalidBindingsJson)?;
    if document.schema_version != 1 {
        return Err(ConfigError::UnsupportedBindingsSchema {
            found: document.schema_version,
        });
    }
    if document.channel_bindings.is_empty() {
        return Err(ConfigError::MissingChannelBinding);
    }
    let mut keys = BTreeSet::new();
    let mut ids = BTreeSet::new();
    let mut bindings = ResourceBindingMap::default();
    for entry in document.channel_bindings {
        let (key, id) = parse_binding_entry::<ChannelId>(entry, &mut keys, &mut ids)?;
        bindings.channel_bindings.insert(key, id);
    }
    for entry in document.role_bindings {
        let (key, id) = parse_binding_entry::<RoleId>(entry, &mut keys, &mut ids)?;
        bindings.role_bindings.insert(key, id);
    }
    Ok(bindings)
}

fn parse_binding_entry<I>(
    entry: BindingEntryV1,
    keys: &mut BTreeSet<String>,
    ids: &mut BTreeSet<u64>,
) -> Result<(ResourceKey, I), ConfigError>
where
    I: std::str::FromStr,
{
    let key = entry.key;
    let valid_key = !key.is_empty()
        && key.chars().count() <= 64
        && key.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | ':' | '/')
        });
    if !valid_key {
        return Err(ConfigError::InvalidBindingKey);
    }
    if !keys.insert(key.clone()) {
        return Err(ConfigError::DuplicateBindingKey);
    }
    let numeric_id = entry
        .id
        .parse::<u64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or(ConfigError::InvalidBindingId)?;
    if !ids.insert(numeric_id) {
        return Err(ConfigError::DuplicateBindingId);
    }
    let id = entry
        .id
        .parse::<I>()
        .map_err(|_| ConfigError::InvalidBindingId)?;
    Ok((ResourceKey(key), id))
}

#[derive(Debug)]
pub enum ConfigError {
    BindingsRequireIntentMode,
    ConflictingHarnessModes,
    DuplicateBindingId,
    DuplicateBindingKey,
    EmptyDatabasePath,
    EmptyBaseUrl,
    EmptyCodexWorkerUrl,
    EmptyModel,
    EmptySessionId,
    InvalidBindingId,
    InvalidBindingKey,
    InvalidBindingsJson,
    InvalidBound { name: &'static str },
    InvalidHarnessMode,
    InvalidPlannedMode,
    MissingApiKey,
    MissingCodexWorkerToken,
    MissingChannelBinding,
    MissingHomeForDefaultDatabasePath,
    MissingIntentBindings,
    UnsupportedBindingsSchema { found: u32 },
    UnsupportedModel,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BindingsRequireIntentMode => formatter.write_str(
                "STARRING_HARNESS_BINDINGS_JSON is only valid in intent_recipe mode",
            ),
            Self::ConflictingHarnessModes => formatter.write_str(
                "STARRING_HARNESS_MODE and STARRING_HARNESS_PLANNED cannot both be set",
            ),
            Self::DuplicateBindingId => {
                formatter.write_str("resource binding Discord IDs must be unique")
            }
            Self::DuplicateBindingKey => {
                formatter.write_str("resource binding keys must be unique")
            }
            Self::EmptyDatabasePath => {
                formatter.write_str("STARRING_HARNESS_DB_PATH must not be empty")
            }
            Self::EmptyBaseUrl => formatter.write_str("STARRING_LLM_BASE_URL must not be empty"),
            Self::EmptyCodexWorkerUrl => {
                formatter.write_str("STARRING_CODEX_WORKER_URL must not be empty")
            }
            Self::EmptyModel => formatter.write_str("STARRING_LLM_MODEL must not be empty"),
            Self::EmptySessionId => {
                formatter.write_str("STARRING_HARNESS_SESSION_ID must not be empty")
            }
            Self::InvalidBindingId => formatter.write_str(
                "resource binding IDs must be non-zero decimal Discord ID strings",
            ),
            Self::InvalidBindingKey => formatter.write_str(
                "resource binding keys must contain 1 to 64 ASCII letters, digits, _, -, ., :, or /",
            ),
            Self::InvalidBindingsJson => formatter.write_str(
                "STARRING_HARNESS_BINDINGS_JSON must be a strict binding document",
            ),
            Self::InvalidBound { name } => write!(formatter, "{name} must be a positive integer"),
            Self::InvalidHarnessMode => formatter.write_str(
                "STARRING_HARNESS_MODE must be adaptive, typed_plan, or intent_recipe",
            ),
            Self::InvalidPlannedMode => {
                formatter.write_str("STARRING_HARNESS_PLANNED must be 0, 1, false, or true")
            }
            Self::MissingApiKey => formatter.write_str("STARRING_LLM_API_KEY is required"),
            Self::MissingCodexWorkerToken => {
                formatter.write_str("STARRING_CODEX_WORKER_TOKEN is required")
            }
            Self::MissingChannelBinding => formatter.write_str(
                "intent_recipe mode requires at least one existing channel binding",
            ),
            Self::MissingHomeForDefaultDatabasePath => formatter.write_str(
                "HOME or STARRING_HARNESS_DB_PATH is required for interactive persistence",
            ),
            Self::MissingIntentBindings => formatter.write_str(
                "STARRING_HARNESS_BINDINGS_JSON is required in intent_recipe mode",
            ),
            Self::UnsupportedBindingsSchema { found } => write!(
                formatter,
                "unsupported resource binding schema version {found}; expected 1"
            ),
            Self::UnsupportedModel => write!(
                formatter,
                "STARRING_LLM_MODEL must be {LEGACY_SERVING_MODEL}"
            ),
        }
    }
}

impl Error for ConfigError {}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        codex_worker_config_from, edge_config_from, parse_bindings, persistence_config_from,
        session_config_from, ConfigError, HarnessMode, LEGACY_SERVING_MODEL,
    };

    #[test]
    fn edge_config_is_gemma_only_and_keeps_secrets_out_of_errors() {
        let defaults = edge_config_from(|name| {
            (name == "STARRING_LLM_API_KEY").then(|| "test-secret".to_string())
        })
        .unwrap();
        assert_eq!(defaults.model, LEGACY_SERVING_MODEL);
        assert_eq!(defaults.base_url, "https://llm-api.starring.co.kr/v1");

        let unsupported = match edge_config_from(|name| match name {
            "STARRING_LLM_API_KEY" => Some("secret-marker".to_string()),
            "STARRING_LLM_MODEL" => Some("qwen3.5:9b-mlx".to_string()),
            _ => None,
        }) {
            Err(error) => error,
            Ok(_) => panic!("unsupported model was accepted"),
        };
        assert!(matches!(unsupported, ConfigError::UnsupportedModel));
        let message = unsupported.to_string();
        assert!(!message.contains("secret-marker"));
        assert!(!message.contains("qwen"));

        assert!(matches!(
            edge_config_from(|_| None),
            Err(ConfigError::MissingApiKey)
        ));
    }

    #[test]
    fn codex_worker_config_pins_loopback_and_keeps_token_out_of_errors() {
        let defaults = codex_worker_config_from(|name| {
            (name == "STARRING_CODEX_WORKER_TOKEN").then(|| "test-secret".to_string())
        })
        .unwrap();
        assert_eq!(defaults.base_url, "http://127.0.0.1:18181");
        assert_eq!(defaults.token, "test-secret");

        let empty_url = match codex_worker_config_from(|name| match name {
            "STARRING_CODEX_WORKER_TOKEN" => Some("secret-marker".to_string()),
            "STARRING_CODEX_WORKER_URL" => Some(String::new()),
            _ => None,
        }) {
            Err(error) => error,
            Ok(_) => panic!("empty worker URL was accepted"),
        };
        assert!(matches!(empty_url, ConfigError::EmptyCodexWorkerUrl));
        assert!(!empty_url.to_string().contains("secret-marker"));

        assert!(matches!(
            codex_worker_config_from(|_| None),
            Err(ConfigError::MissingCodexWorkerToken)
        ));
    }

    #[test]
    fn session_bounds_use_defaults_when_env_is_absent() {
        let config = session_config_from(|_| None).unwrap();

        assert_eq!(config.max_model_calls, 12);
        assert_eq!(config.max_tool_calls, 24);
        assert_eq!(config.max_gate_failures, 4);
        assert_eq!(config.context_char_budget, 44_000);
    }

    #[test]
    fn session_bounds_read_all_env_overrides() {
        let values = BTreeMap::from([
            ("STARRING_HARNESS_MAX_MODEL_CALLS", "20"),
            ("STARRING_HARNESS_MAX_TOOL_CALLS", "40"),
            ("STARRING_HARNESS_MAX_GATE_FAILURES", "8"),
            ("STARRING_HARNESS_CONTEXT_CHARS", "32000"),
        ]);
        let config = session_config_from(|name| values.get(name).map(ToString::to_string)).unwrap();

        assert_eq!(config.max_model_calls, 20);
        assert_eq!(config.max_tool_calls, 40);
        assert_eq!(config.max_gate_failures, 8);
        assert_eq!(config.context_char_budget, 32_000);
    }

    #[test]
    fn session_bounds_reject_zero_and_non_numeric_values() {
        let zero = session_config_from(|name| {
            (name == "STARRING_HARNESS_MAX_TOOL_CALLS").then(|| "0".to_string())
        });
        let non_numeric = session_config_from(|name| {
            (name == "STARRING_HARNESS_CONTEXT_CHARS").then(|| "large".to_string())
        });

        assert!(matches!(
            zero,
            Err(ConfigError::InvalidBound {
                name: "STARRING_HARNESS_MAX_TOOL_CALLS"
            })
        ));
        assert!(matches!(
            non_numeric,
            Err(ConfigError::InvalidBound {
                name: "STARRING_HARNESS_CONTEXT_CHARS"
            })
        ));
    }

    #[test]
    fn persistence_defaults_under_home_and_accepts_overrides() {
        let defaults =
            persistence_config_from(|name| (name == "HOME").then(|| "/home/tester".to_string()))
                .unwrap();
        assert_eq!(defaults.session_id, "default");
        assert_eq!(defaults.mode, HarnessMode::Adaptive);
        assert!(defaults.bindings.is_none());
        assert_eq!(
            defaults.db_path,
            std::path::Path::new("/home/tester/.local/share/starring/design-harness.sqlite3")
        );

        let values = BTreeMap::from([
            ("STARRING_HARNESS_DB_PATH", "/tmp/harness.db"),
            ("STARRING_HARNESS_SESSION_ID", "study-room"),
            ("STARRING_HARNESS_PLANNED", "true"),
        ]);
        let custom =
            persistence_config_from(|name| values.get(name).map(ToString::to_string)).unwrap();
        assert_eq!(custom.db_path, std::path::Path::new("/tmp/harness.db"));
        assert_eq!(custom.session_id, "study-room");
        assert_eq!(custom.mode, HarnessMode::TypedPlan);
    }

    #[test]
    fn persistence_rejects_empty_values_and_missing_home() {
        assert!(matches!(
            persistence_config_from(|_| None),
            Err(ConfigError::MissingHomeForDefaultDatabasePath)
        ));
        assert!(matches!(
            persistence_config_from(|name| match name {
                "HOME" => Some("/home/tester".to_string()),
                "STARRING_HARNESS_SESSION_ID" => Some(String::new()),
                _ => None,
            }),
            Err(ConfigError::EmptySessionId)
        ));
        assert!(matches!(
            persistence_config_from(|name| match name {
                "STARRING_HARNESS_DB_PATH" => Some(String::new()),
                _ => None,
            }),
            Err(ConfigError::EmptyDatabasePath)
        ));
        assert!(matches!(
            persistence_config_from(|name| match name {
                "HOME" => Some("/home/tester".to_string()),
                "STARRING_HARNESS_PLANNED" => Some("yes".to_string()),
                _ => None,
            }),
            Err(ConfigError::InvalidPlannedMode)
        ));
    }

    #[test]
    fn planned_mode_accepts_only_the_four_exact_boolean_forms() {
        for (value, expected) in [("1", true), ("true", true), ("0", false), ("false", false)] {
            let config = persistence_config_from(|name| match name {
                "STARRING_HARNESS_DB_PATH" => Some("/tmp/harness.db".to_string()),
                "STARRING_HARNESS_PLANNED" => Some(value.to_string()),
                _ => None,
            })
            .unwrap();
            assert_eq!(
                config.mode,
                if expected {
                    HarnessMode::TypedPlan
                } else {
                    HarnessMode::Adaptive
                }
            );
        }
        for value in ["True", "FALSE", " true", "false "] {
            assert!(matches!(
                persistence_config_from(|name| match name {
                    "STARRING_HARNESS_DB_PATH" => Some("/tmp/harness.db".to_string()),
                    "STARRING_HARNESS_PLANNED" => Some(value.to_string()),
                    _ => None,
                }),
                Err(ConfigError::InvalidPlannedMode)
            ));
        }
    }

    #[test]
    fn explicit_modes_are_exact_and_conflict_with_the_legacy_switch() {
        for (value, expected) in [
            ("adaptive", HarnessMode::Adaptive),
            ("typed_plan", HarnessMode::TypedPlan),
        ] {
            let config = persistence_config_from(|name| match name {
                "STARRING_HARNESS_DB_PATH" => Some("/tmp/harness.db".to_string()),
                "STARRING_HARNESS_MODE" => Some(value.to_string()),
                _ => None,
            })
            .unwrap();
            assert_eq!(config.mode, expected);
        }
        for value in ["", "planned", "Intent_Recipe", " adaptive"] {
            assert!(matches!(
                persistence_config_from(|name| match name {
                    "STARRING_HARNESS_DB_PATH" => Some("/tmp/harness.db".to_string()),
                    "STARRING_HARNESS_MODE" => Some(value.to_string()),
                    _ => None,
                }),
                Err(ConfigError::InvalidHarnessMode)
            ));
        }
        assert!(matches!(
            persistence_config_from(|name| match name {
                "STARRING_HARNESS_DB_PATH" => Some("/tmp/harness.db".to_string()),
                "STARRING_HARNESS_MODE" => Some("adaptive".to_string()),
                "STARRING_HARNESS_PLANNED" => Some("false".to_string()),
                _ => None,
            }),
            Err(ConfigError::ConflictingHarnessModes)
        ));
    }

    #[test]
    fn intent_mode_requires_and_builds_one_strict_binding_map() {
        let document = r#"{
            "schema_version":1,
            "channel_bindings":[{"key":"study_hub","id":"700"}],
            "role_bindings":[{"key":"member","id":"701"}]
        }"#;
        let config = persistence_config_from(|name| match name {
            "STARRING_HARNESS_DB_PATH" => Some("/tmp/harness.db".to_string()),
            "STARRING_HARNESS_MODE" => Some("intent_recipe".to_string()),
            "STARRING_HARNESS_BINDINGS_JSON" => Some(document.to_string()),
            _ => None,
        })
        .unwrap();

        assert_eq!(config.mode, HarnessMode::IntentRecipe);
        let bindings = config.bindings.unwrap();
        assert_eq!(bindings.channel_bindings.len(), 1);
        assert_eq!(bindings.role_bindings.len(), 1);
        assert_eq!(
            bindings
                .channel_bindings
                .iter()
                .next()
                .map(|(key, id)| (key.0.as_str(), id.to_string())),
            Some(("study_hub", "700".to_string()))
        );
    }

    #[test]
    fn binding_documents_reject_ambiguous_or_unsafe_values() {
        for document in [
            r#"{"schema_version":2,"channel_bindings":[{"key":"hub","id":"1"}]}"#,
            r#"{"schema_version":1,"channel_bindings":[]}"#,
            r#"{"schema_version":1,"channel_bindings":[{"key":"bad key","id":"1"}]}"#,
            r#"{"schema_version":1,"channel_bindings":[{"key":"hub","id":"0"}]}"#,
            r#"{"schema_version":1,"channel_bindings":[{"key":"hub","id":"1"},{"key":"hub","id":"2"}]}"#,
            r#"{"schema_version":1,"channel_bindings":[{"key":"hub","id":"1"}],"role_bindings":[{"key":"member","id":"1"}]}"#,
            r#"{"schema_version":1,"channel_bindings":[{"key":"hub","id":"1","extra":true}]}"#,
            r#"{"schema_version":1,"channel_bindings":[{"key":"hub","id":1}]}"#,
        ] {
            assert!(parse_bindings(document).is_err(), "accepted {document}");
        }
    }

    #[test]
    fn bindings_are_rejected_outside_intent_mode_and_missing_inside_it() {
        assert!(matches!(
            persistence_config_from(|name| match name {
                "STARRING_HARNESS_DB_PATH" => Some("/tmp/harness.db".to_string()),
                "STARRING_HARNESS_BINDINGS_JSON" => Some("{}".to_string()),
                _ => None,
            }),
            Err(ConfigError::BindingsRequireIntentMode)
        ));
        assert!(matches!(
            persistence_config_from(|name| match name {
                "STARRING_HARNESS_DB_PATH" => Some("/tmp/harness.db".to_string()),
                "STARRING_HARNESS_MODE" => Some("intent_recipe".to_string()),
                _ => None,
            }),
            Err(ConfigError::MissingIntentBindings)
        ));
    }
}
