mod authoring_admission;
mod composition;
mod config;
mod error;
mod facade;
mod input;
mod projection;
mod secret;
mod server;

pub use authoring_admission::{
    AuthoringAdmissionConfigErrorV1, AuthoringAdmissionConfigV1, AuthoringAdmissionV1,
};
pub use composition::{
    compose_production_service_v1, ComposedProductionServiceV1, ProductionCompositionErrorV1,
    ProductionDatabasePoolShutdownErrorV1, ProductionDatabasePoolShutdownV1,
    ProductionReadinessPhaseV1,
};
pub use config::{
    DatabaseRoleV1, ProductionConfigErrorV1, ProductionConfigV1, ProductionConfigurationFieldV1,
};
pub use error::{
    map_authentication_error, map_authoring_application_error, map_database_failure,
    map_discord_oauth_error, map_fresh_authority_error, map_oauth_flow_error,
    map_product_application_error, map_product_control_error, map_product_identity_error,
};
pub use facade::{
    ProductionAuthorityDependenciesV1, ProductionFacadeConfigurationErrorV1,
    ProductionIdentityDependenciesV1, ProductionPersistenceDependenciesV1,
    ProductionProductControlFacadeV1,
};
pub use input::{
    map_apply_command, map_approve_command, map_discord_authorization_code,
    map_discord_oauth_state, map_lifecycle_cancellation_command, map_product_target,
    map_promote_command, map_reject_command, MappedApplyCommand, MappedApproveCommand,
    MappedLifecycleCancellationCommand, MappedProductTarget, MappedPromoteCommand,
    MappedRejectCommand,
};
pub use projection::{
    project_apply, project_approval_preview, project_current_principal, project_decision_mutation,
    project_deployment, project_deployment_operational_v2, project_lifecycle_cancellation,
    project_oauth_callback, project_oauth_start, project_product_status, project_promotion,
};
pub use secret::{
    resolve_production_secrets_v1, KeyringPayloadErrorV1, ProductionSecretResolutionErrorV1,
    ResolvedProductionSecretsV1, SecretResolutionErrorV1,
};
pub use server::{
    serve_verified_loopback, serve_verified_loopback_with_runtime_readiness, LoopbackServeErrorV1,
    LoopbackServeReportV1, LoopbackServerConfigErrorV1, LoopbackServerConfigInputV1,
    LoopbackServerConfigV1, MAX_GRACEFUL_DRAIN_TIMEOUT,
};
