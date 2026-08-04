use serde_json::Value;
use sqlx::types::Json;

#[derive(sqlx::FromRow)]
pub(super) struct AuthoringWriterCheckRowV1 {
    pub outcome_code: String,
    pub current_generation: Option<i64>,
    pub matched_generation: Option<i64>,
    pub safe_turn_projection: Option<Vec<u8>>,
    pub safe_turn_projection_digest: Option<String>,
}

#[derive(sqlx::FromRow)]
pub(super) struct AuthoringWriterLoadRowV1 {
    pub outcome_code: String,
    pub head_generation: Option<i64>,
    pub snapshot_schema_version: Option<i64>,
    pub snapshot_ciphertext: Option<Vec<u8>>,
    pub snapshot_nonce: Option<Vec<u8>>,
    pub encryption_key_id: Option<String>,
    pub encryption_suite: Option<String>,
    pub encryption_suite_version: Option<i16>,
    pub authenticated_metadata_digest: Option<String>,
    pub resource_bindings: Option<Json<Value>>,
    pub binding_fingerprint: Option<String>,
    pub installation_authority_revision: Option<i64>,
    pub authority_payload_digest: Option<String>,
    pub writer_request_digest: Option<String>,
    pub writer_semantic_request_digest: Option<String>,
    pub writer_digest_key_id: Option<String>,
    pub writer_digest_key_fingerprint: Option<String>,
    pub safe_turn_projection: Option<Vec<u8>>,
    pub safe_turn_projection_digest: Option<String>,
    pub stage: Option<String>,
    pub candidate_revision: Option<i64>,
    pub candidate_hash: Option<String>,
    pub harness_contract_revision: Option<i64>,
    pub current_authority_revision: Option<i64>,
    pub current_authority_payload_digest: Option<String>,
    pub current_resource_bindings: Option<Json<Value>>,
    pub current_binding_fingerprint: Option<String>,
}

#[derive(sqlx::FromRow)]
pub(super) struct AuthoringWriterCommitRowV1 {
    pub outcome_code: String,
    pub current_generation: Option<i64>,
    pub committed_generation: Option<i64>,
    pub safe_turn_projection: Option<Vec<u8>>,
    pub safe_turn_projection_digest: Option<String>,
}

#[derive(sqlx::FromRow)]
pub(super) struct AuthoringWriterCoverageRowV1 {
    pub covered: bool,
}
