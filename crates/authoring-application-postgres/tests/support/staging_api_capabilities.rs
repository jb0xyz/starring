pub struct StagingApiCapabilitySet {
    pub fixture_label: &'static str,
    pub staging_role: &'static str,
    pub functions: &'static [&'static str],
}

const OAUTH_FLOW_FUNCTIONS: &[&str] = &[
    "public.starring_product_oauth_database_identity_v1()",
    "public.starring_product_oauth_flow_create_v1(bytea,bytea,text,text,double precision)",
    "public.starring_product_oauth_flow_consume_v1(bytea,bytea,text,text[])",
];
const SESSION_ISSUER_FUNCTIONS: &[&str] = &[
    "public.starring_product_session_issuer_database_identity_v1()",
    "public.starring_product_session_issue_v1(bytea,text,text,timestamp with time zone,text,text,bytea,bytea,double precision,double precision)",
];
const SESSION_API_FUNCTIONS: &[&str] = &[
    "public.starring_product_session_api_database_identity_v1()",
    "public.starring_product_session_read_v1(bytea)",
    "public.starring_product_session_mutation_read_v1(bytea)",
    "public.starring_product_session_touch_v1(bytea,timestamp with time zone,timestamp with time zone,timestamp with time zone,double precision)",
    "public.starring_product_session_logout_read_v1(bytea)",
    "public.starring_product_session_logout_commit_v1(bytea,bytea,timestamp with time zone)",
];
const SECURITY_REVOKER_FUNCTIONS: &[&str] = &[
    "public.starring_product_security_revoker_database_identity_v1()",
    "public.starring_product_session_security_revoke_v1(bytea)",
];
const INSTALLATION_AUTHORITY_FUNCTIONS: &[&str] = &[
    "public.starring_product_installation_authority_reader_database_identity_v1()",
    "public.starring_product_installation_authority_read_v1(text,text,bytea)",
];
const AUTHORIZED_SNAPSHOT_FUNCTIONS: &[&str] = &[
    "public.starring_product_authorized_snapshot_reader_database_identity_v1()",
    "public.starring_product_authorized_snapshot_read_v2(text,text,bytea,text,text)",
    "public.starring_product_authorized_snapshot_key_coverage_v1(text[])",
];
const PROMOTION_FUNCTIONS: &[&str] = &[
    "public.starring_product_promotion_executor_database_identity_v1()",
    "public.starring_product_promotion_replay_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,bigint,text,text[],text[],text[])",
    "public.starring_product_promotion_prepare_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bytea,text,bigint,bigint,text,text,text,text,jsonb,jsonb,text,text,text[],text[],text[],text,text,text,text)",
    "public.starring_product_promotion_publish_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text)",
    "public.starring_product_promotion_approval_environment_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text)",
    "public.starring_product_promotion_activation_link_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text,jsonb)",
    "public.starring_product_promotion_repair_link_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text,bytea,jsonb,text,text,text[],text[],text[],text,text,text,text)",
    "public.starring_product_promotion_keyring_coverage_v1(text[],text[])",
];
const DECISION_READER_FUNCTIONS: &[&str] = &[
    "public.starring_product_decision_reader_database_identity_v1()",
    "public.starring_product_decision_read_v1(text,text,text,text,text,text,bytea)",
];
const APPROVAL_EXECUTOR_FUNCTIONS: &[&str] = &[
    "public.starring_product_approval_executor_database_identity_v1()",
    "public.starring_product_approve_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text)",
    "public.starring_product_approval_keyring_coverage_v1(text[],text[])",
];
const REJECTION_EXECUTOR_FUNCTIONS: &[&str] = &[
    "public.starring_product_rejection_executor_database_identity_v1()",
    "public.starring_product_rejection_keyring_coverage_v1(text[],text[])",
    "public.starring_product_reject_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text)",
];
const APPLY_EXECUTOR_FUNCTIONS: &[&str] = &[
    "public.starring_product_apply_executor_database_identity_v1()",
    "public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)",
    "public.starring_product_apply_target_artifact_v1(text,text,text,text,bytea,text,text)",
    "public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)",
    "public.starring_product_apply_keyring_coverage_v1(text[],text[])",
    "public.starring_product_apply_begin_runtime_drain_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text,text)",
    "public.starring_product_apply_consume_runtime_drain_v2(text,text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text,bigint,bytea,text,text,text,bigint,text,text,bytea,text,bytea,text,text,bytea)",
];
const CANCELLATION_EXECUTOR_FUNCTIONS: &[&str] = &[
    "public.starring_product_lifecycle_cancellation_executor_database_identity_v1()",
    "public.starring_product_lifecycle_cancellation_keyring_coverage_v1(text[],text[])",
    "public.starring_product_cancel_runtime_drain_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text,text,bigint,text,text,bigint)",
];
const DEPLOYMENT_STATUS_FUNCTIONS: &[&str] = &[
    "public.starring_product_deployment_status_reader_database_identity_v1()",
    "public.starring_product_deployment_status_read_v1(text,text,text,text,text,text,text,text,bytea)",
];
pub const OPERATIONAL_DEPLOYMENT_STATUS_FUNCTIONS: &[&str] = &[
    "public.starring_product_deployment_status_reader_database_identity_v2()",
    "public.starring_product_deployment_status_read_v2(text,text,text,text,text,text,text,text,bytea)",
];
const AUTHORING_SESSION_WRITER_FUNCTIONS: &[&str] = &[
    "public.starring_authoring_session_writer_database_identity_v1()",
    "public.starring_authoring_session_writer_check_v1(text,text,text,text,bigint,text[],text[],text[],text[])",
    "public.starring_authoring_session_writer_load_v1(text,text,text,text,bigint)",
    "public.starring_authoring_session_writer_commit_v1(text,text,text,text,bigint,text[],text[],text[],text[],text,text,text,text,bigint,bytea,bytea,text,text,smallint,text,jsonb,text,bigint,text,jsonb,text,bigint,text,bytea,text,bigint)",
    "public.starring_authoring_session_writer_key_coverage_v1(text[],text[],text[])",
];

pub const CAPABILITIES: [StagingApiCapabilitySet; 15] = [
    StagingApiCapabilitySet {
        fixture_label: "oauth",
        staging_role: "starring_identity_oauth",
        functions: OAUTH_FLOW_FUNCTIONS,
    },
    StagingApiCapabilitySet {
        fixture_label: "issuer",
        staging_role: "starring_identity_issuer",
        functions: SESSION_ISSUER_FUNCTIONS,
    },
    StagingApiCapabilitySet {
        fixture_label: "session",
        staging_role: "starring_identity_session",
        functions: SESSION_API_FUNCTIONS,
    },
    StagingApiCapabilitySet {
        fixture_label: "revoker",
        staging_role: "starring_identity_security",
        functions: SECURITY_REVOKER_FUNCTIONS,
    },
    StagingApiCapabilitySet {
        fixture_label: "authority",
        staging_role: "starring_installation_authority_reader",
        functions: INSTALLATION_AUTHORITY_FUNCTIONS,
    },
    StagingApiCapabilitySet {
        fixture_label: "snapshot",
        staging_role: "starring_authorized_snapshot_reader",
        functions: AUTHORIZED_SNAPSHOT_FUNCTIONS,
    },
    StagingApiCapabilitySet {
        fixture_label: "promotion",
        staging_role: "starring_promotion_executor",
        functions: PROMOTION_FUNCTIONS,
    },
    StagingApiCapabilitySet {
        fixture_label: "decision",
        staging_role: "starring_decision_reader",
        functions: DECISION_READER_FUNCTIONS,
    },
    StagingApiCapabilitySet {
        fixture_label: "approval",
        staging_role: "starring_decision_approval",
        functions: APPROVAL_EXECUTOR_FUNCTIONS,
    },
    StagingApiCapabilitySet {
        fixture_label: "rejection",
        staging_role: "starring_decision_rejection",
        functions: REJECTION_EXECUTOR_FUNCTIONS,
    },
    StagingApiCapabilitySet {
        fixture_label: "apply",
        staging_role: "starring_decision_apply",
        functions: APPLY_EXECUTOR_FUNCTIONS,
    },
    StagingApiCapabilitySet {
        fixture_label: "cancellation",
        staging_role: "starring_decision_cancellation",
        functions: CANCELLATION_EXECUTOR_FUNCTIONS,
    },
    StagingApiCapabilitySet {
        fixture_label: "status",
        staging_role: "starring_deployment_status_reader",
        functions: DEPLOYMENT_STATUS_FUNCTIONS,
    },
    StagingApiCapabilitySet {
        fixture_label: "operational",
        staging_role: "starring_operational_deployment_status_reader",
        functions: OPERATIONAL_DEPLOYMENT_STATUS_FUNCTIONS,
    },
    StagingApiCapabilitySet {
        fixture_label: "authoring_writer",
        staging_role: "starring_authoring_session_writer",
        functions: AUTHORING_SESSION_WRITER_FUNCTIONS,
    },
];
