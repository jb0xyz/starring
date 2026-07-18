use automation_ruleset_activation::{
    ActivationApprovalContextV1, ActivationDigest, ActivationLinkStateV1, ActivationRequest,
    ActivationRequestId, ActivationRequestState, ActivationStoreError, ActivationTarget,
    ActivationTerminationV1, ApplyAttemptId, ApplyErrorRecord, Approval, Completion,
    CompletionKind, ObservedActive, Rejection,
};
use chrono::{DateTime, Utc};
use discord_model::{GuildId, UserId};
use sqlx::types::Json;

pub(crate) const REQUEST_COLUMNS: &str = "id, guild_id, ruleset_key, target_version, target_content_hash, requester_id, required_approvals, state, created_at, expires_at, apply_attempt_id, apply_attempt_no, apply_lease_until, last_apply_error, observed_active_version, observed_active_hash, applied_at, applied_by, completion_kind, activation_notices, rejected_at, rejected_by, rejection_reason, authority_kind, link_state_name, approval_context, link_state, promotion_id, promotion_request_digest, approval_payload_digest, approval_context_digest, linked_at, termination";

#[derive(Clone, sqlx::FromRow)]
pub(crate) struct ActivationRequestRow {
    pub id: String,
    pub guild_id: String,
    pub ruleset_key: String,
    pub target_version: i64,
    pub target_content_hash: String,
    pub requester_id: String,
    pub required_approvals: i32,
    pub state: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub apply_attempt_id: Option<String>,
    pub apply_attempt_no: i64,
    pub apply_lease_until: Option<DateTime<Utc>>,
    pub last_apply_error: Option<Json<ApplyErrorRecord>>,
    pub observed_active_version: Option<i64>,
    pub observed_active_hash: Option<String>,
    pub applied_at: Option<DateTime<Utc>>,
    pub applied_by: Option<String>,
    pub completion_kind: Option<String>,
    pub activation_notices: Option<Json<Vec<String>>>,
    pub rejected_at: Option<DateTime<Utc>>,
    pub rejected_by: Option<String>,
    pub rejection_reason: Option<String>,
    pub authority_kind: String,
    pub link_state_name: String,
    pub approval_context: Json<ActivationApprovalContextV1>,
    pub link_state: Json<ActivationLinkStateV1>,
    pub promotion_id: Option<String>,
    pub promotion_request_digest: Option<String>,
    pub approval_payload_digest: Option<String>,
    pub approval_context_digest: Option<String>,
    pub linked_at: Option<DateTime<Utc>>,
    pub termination: Option<Json<ActivationTerminationV1>>,
}

#[derive(Clone, sqlx::FromRow)]
pub(crate) struct ApprovalRow {
    pub approver_id: String,
    pub approved_at: DateTime<Utc>,
    pub approval_payload_digest: Option<String>,
}

pub(crate) fn backend(error: impl std::fmt::Display) -> ActivationStoreError {
    ActivationStoreError::Backend(error.to_string())
}

pub(crate) fn decode_request(
    row: ActivationRequestRow,
    approvals: Vec<ApprovalRow>,
) -> Result<ActivationRequest, ActivationStoreError> {
    let id = ActivationRequestId::parse(&row.id)
        .map_err(|error| backend(format!("invalid persisted id: {error}")))?;
    let guild_id = row
        .guild_id
        .parse::<GuildId>()
        .map_err(|_| backend(format!("invalid persisted guild_id: {}", row.guild_id)))?;
    let ruleset_key = automation_ruleset::RuleSetKey::parse(&row.ruleset_key)
        .map_err(|error| backend(format!("invalid persisted ruleset_key: {error:?}")))?;
    let version = u32::try_from(row.target_version)
        .ok()
        .and_then(|value| automation_ruleset::RuleSetVersionId::new(value).ok())
        .ok_or_else(|| {
            backend(format!(
                "invalid persisted target_version: {}",
                row.target_version
            ))
        })?;
    let content_hash = automation_ruleset::RuleSetContentHash::parse_hex(&row.target_content_hash)
        .ok_or_else(|| {
            backend(format!(
                "invalid persisted target_content_hash: {}",
                row.target_content_hash
            ))
        })?;
    let requester = parse_user(&row.requester_id, "requester_id")?;
    let required_approvals = u32::try_from(row.required_approvals)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            backend(format!(
                "invalid persisted required_approvals: {}",
                row.required_approvals
            ))
        })?;
    let state = parse_state(&row.state)?;
    let apply_attempt_id = row
        .apply_attempt_id
        .as_deref()
        .map(ApplyAttemptId::parse)
        .transpose()
        .map_err(|error| backend(format!("invalid persisted apply_attempt_id: {error}")))?;
    let apply_attempt_no = u64::try_from(row.apply_attempt_no).map_err(|_| {
        backend(format!(
            "invalid persisted apply_attempt_no: {}",
            row.apply_attempt_no
        ))
    })?;
    if (state == ActivationRequestState::Applying)
        != (apply_attempt_id.is_some() && row.apply_lease_until.is_some())
    {
        return Err(backend("invalid persisted applying fields"));
    }
    if state != ActivationRequestState::Applying
        && (apply_attempt_id.is_some() || row.apply_lease_until.is_some())
    {
        return Err(backend("invalid persisted non-applying fields"));
    }
    if row.expires_at <= row.created_at {
        return Err(backend("invalid persisted expiry"));
    }
    let observed_active = decode_observed(
        row.observed_active_version,
        row.observed_active_hash.as_deref(),
    )?;
    let completion = decode_completion(
        state,
        row.applied_at,
        row.applied_by.as_deref(),
        row.completion_kind.as_deref(),
        row.activation_notices.map(|notices| notices.0),
    )?;
    let rejection = decode_rejection(
        state,
        row.rejected_at,
        row.rejected_by.as_deref(),
        row.rejection_reason,
    )?;
    let approval_context = row.approval_context.0;
    let link_state = row.link_state.0;
    validate_context_shadows(
        &approval_context,
        &link_state,
        ContextShadows {
            authority_kind: &row.authority_kind,
            link_state_name: &row.link_state_name,
            promotion_id: row.promotion_id.as_deref(),
            promotion_request_digest: row.promotion_request_digest.as_deref(),
            approval_payload_digest: row.approval_payload_digest.as_deref(),
            approval_context_digest: row.approval_context_digest.as_deref(),
            linked_at: row.linked_at,
        },
    )?;
    let mut approvals = approvals
        .into_iter()
        .map(|approval| {
            Ok(Approval {
                approver: parse_user(&approval.approver_id, "approver_id")?,
                approved_at: approval.approved_at,
                approval_payload_digest: approval
                    .approval_payload_digest
                    .as_deref()
                    .map(ActivationDigest::parse)
                    .transpose()
                    .map_err(|error| {
                        backend(format!(
                            "invalid persisted approval payload digest: {error}"
                        ))
                    })?,
            })
        })
        .collect::<Result<Vec<_>, ActivationStoreError>>()?;
    approvals.sort_by_key(|approval| approval.approver);
    let request = ActivationRequest {
        id,
        target: ActivationTarget {
            guild_id,
            ruleset_key,
            version,
            content_hash,
        },
        requester,
        required_approvals,
        approval_context,
        link_state,
        approvals,
        state,
        rejection,
        apply_attempt_id,
        apply_attempt_no,
        apply_lease_until: row.apply_lease_until,
        last_apply_error: row.last_apply_error.map(|error| error.0),
        observed_active,
        completion,
        termination: row.termination.map(|termination| termination.0),
        created_at: row.created_at,
        expires_at: row.expires_at,
    };
    request
        .validate()
        .map_err(|error| backend(format!("invalid persisted activation request: {error}")))?;
    Ok(request)
}

struct ContextShadows<'a> {
    authority_kind: &'a str,
    link_state_name: &'a str,
    promotion_id: Option<&'a str>,
    promotion_request_digest: Option<&'a str>,
    approval_payload_digest: Option<&'a str>,
    approval_context_digest: Option<&'a str>,
    linked_at: Option<DateTime<Utc>>,
}

fn validate_context_shadows(
    approval_context: &ActivationApprovalContextV1,
    link_state: &ActivationLinkStateV1,
    shadows: ContextShadows<'_>,
) -> Result<(), ActivationStoreError> {
    match (approval_context, link_state) {
        (ActivationApprovalContextV1::LegacyManual, ActivationLinkStateV1::NotRequired)
            if shadows.authority_kind == "legacy_manual"
                && shadows.link_state_name == "not_required"
                && shadows.promotion_id.is_none()
                && shadows.promotion_request_digest.is_none()
                && shadows.approval_payload_digest.is_none()
                && shadows.approval_context_digest.is_none()
                && shadows.linked_at.is_none() =>
        {
            Ok(())
        }
        (
            ActivationApprovalContextV1::ProductAuthoring { context },
            ActivationLinkStateV1::Unlinked,
        ) if shadows.authority_kind == "product_authoring"
            && shadows.link_state_name == "unlinked"
            && shadows.promotion_id == Some(context.promotion_id.as_str())
            && shadows.promotion_request_digest
                == Some(context.promotion_request_digest.as_str())
            && shadows.approval_payload_digest
                == Some(context.approval_payload_digest.as_str())
            && shadows.approval_context_digest
                == Some(context.approval_context_digest.as_str())
            && shadows.linked_at.is_none() =>
        {
            Ok(())
        }
        (
            ActivationApprovalContextV1::ProductAuthoring { context },
            ActivationLinkStateV1::Linked {
                linked_at: context_linked_at,
            },
        ) if shadows.authority_kind == "product_authoring"
            && shadows.link_state_name == "linked"
            && shadows.promotion_id == Some(context.promotion_id.as_str())
            && shadows.promotion_request_digest
                == Some(context.promotion_request_digest.as_str())
            && shadows.approval_payload_digest
                == Some(context.approval_payload_digest.as_str())
            && shadows.approval_context_digest
                == Some(context.approval_context_digest.as_str())
            && shadows.linked_at == Some(*context_linked_at) =>
        {
            Ok(())
        }
        _ => Err(backend(
            "persisted activation approval context projections do not match",
        )),
    }
}

fn parse_user(value: &str, field: &str) -> Result<UserId, ActivationStoreError> {
    value
        .parse::<UserId>()
        .map_err(|_| backend(format!("invalid persisted {field}: {value}")))
}

fn parse_state(value: &str) -> Result<ActivationRequestState, ActivationStoreError> {
    match value {
        "pending" => Ok(ActivationRequestState::Pending),
        "approved" => Ok(ActivationRequestState::Approved),
        "applying" => Ok(ActivationRequestState::Applying),
        "applied" => Ok(ActivationRequestState::Applied),
        "rejected" => Ok(ActivationRequestState::Rejected),
        "expired" => Ok(ActivationRequestState::Expired),
        "superseded" => Ok(ActivationRequestState::Superseded),
        "withdrawn" => Ok(ActivationRequestState::Withdrawn),
        _ => Err(backend(format!("invalid persisted state: {value}"))),
    }
}

fn decode_observed(
    version: Option<i64>,
    hash: Option<&str>,
) -> Result<Option<ObservedActive>, ActivationStoreError> {
    match (version, hash) {
        (None, None) => Ok(None),
        (Some(version), Some(hash)) => {
            let version = u32::try_from(version)
                .ok()
                .and_then(|value| automation_ruleset::RuleSetVersionId::new(value).ok())
                .ok_or_else(|| backend(format!("invalid persisted observed version: {version}")))?;
            let content_hash = automation_ruleset::RuleSetContentHash::parse_hex(hash)
                .ok_or_else(|| backend(format!("invalid persisted observed hash: {hash}")))?;
            Ok(Some(ObservedActive {
                version,
                content_hash,
            }))
        }
        _ => Err(backend("invalid persisted observed active fields")),
    }
}

fn decode_completion(
    state: ActivationRequestState,
    applied_at: Option<DateTime<Utc>>,
    applied_by: Option<&str>,
    kind: Option<&str>,
    notices: Option<Vec<String>>,
) -> Result<Option<Completion>, ActivationStoreError> {
    match (applied_at, applied_by, kind) {
        (None, None, None) if state != ActivationRequestState::Applied => Ok(None),
        (Some(applied_at), Some(applied_by), Some(kind)) => {
            let kind = match kind {
                "activated" => CompletionKind::Activated,
                "already_active" => CompletionKind::AlreadyActive,
                "crash_recovered" => CompletionKind::CrashRecovered,
                _ => {
                    return Err(backend(format!(
                        "invalid persisted completion kind: {kind}"
                    )))
                }
            };
            Ok(Some(Completion {
                applied_at,
                applied_by: parse_user(applied_by, "applied_by")?,
                kind,
                notices,
            }))
        }
        _ => Err(backend("invalid persisted completion fields")),
    }
}

fn decode_rejection(
    state: ActivationRequestState,
    rejected_at: Option<DateTime<Utc>>,
    rejected_by: Option<&str>,
    reason: Option<String>,
) -> Result<Option<Rejection>, ActivationStoreError> {
    match (rejected_at, rejected_by) {
        (None, None) if state != ActivationRequestState::Rejected => Ok(None),
        (Some(rejected_at), Some(rejected_by)) => Ok(Some(Rejection {
            rejected_at,
            rejected_by: parse_user(rejected_by, "rejected_by")?,
            reason: reason.unwrap_or_default(),
        })),
        _ => Err(backend("invalid persisted rejection fields")),
    }
}

pub(crate) fn state_str(state: ActivationRequestState) -> &'static str {
    match state {
        ActivationRequestState::Pending => "pending",
        ActivationRequestState::Approved => "approved",
        ActivationRequestState::Applying => "applying",
        ActivationRequestState::Applied => "applied",
        ActivationRequestState::Rejected => "rejected",
        ActivationRequestState::Expired => "expired",
        ActivationRequestState::Superseded => "superseded",
        ActivationRequestState::Withdrawn => "withdrawn",
    }
}

pub(crate) fn authority_kind(context: &ActivationApprovalContextV1) -> &'static str {
    match context {
        ActivationApprovalContextV1::LegacyManual => "legacy_manual",
        ActivationApprovalContextV1::ProductAuthoring { .. } => "product_authoring",
    }
}

pub(crate) fn link_state_name(state: &ActivationLinkStateV1) -> &'static str {
    match state {
        ActivationLinkStateV1::NotRequired => "not_required",
        ActivationLinkStateV1::Unlinked => "unlinked",
        ActivationLinkStateV1::Linked { .. } => "linked",
    }
}

pub(crate) fn completion_kind_str(kind: CompletionKind) -> &'static str {
    match kind {
        CompletionKind::Activated => "activated",
        CompletionKind::AlreadyActive => "already_active",
        CompletionKind::CrashRecovered => "crash_recovered",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    fn row() -> ActivationRequestRow {
        let created_at = Utc.with_ymd_and_hms(2026, 7, 12, 0, 0, 0).unwrap();
        ActivationRequestRow {
            id: "request_1".to_string(),
            guild_id: "7".to_string(),
            ruleset_key: "studyroom".to_string(),
            target_version: 1,
            target_content_hash: "11".repeat(32),
            requester_id: "10".to_string(),
            required_approvals: 2,
            state: "pending".to_string(),
            created_at,
            expires_at: created_at + Duration::minutes(30),
            apply_attempt_id: None,
            apply_attempt_no: 0,
            apply_lease_until: None,
            last_apply_error: None,
            observed_active_version: None,
            observed_active_hash: None,
            applied_at: None,
            applied_by: None,
            completion_kind: None,
            activation_notices: None,
            rejected_at: None,
            rejected_by: None,
            rejection_reason: None,
            authority_kind: "legacy_manual".to_string(),
            link_state_name: "not_required".to_string(),
            approval_context: Json(ActivationApprovalContextV1::LegacyManual),
            link_state: Json(ActivationLinkStateV1::NotRequired),
            promotion_id: None,
            promotion_request_digest: None,
            approval_payload_digest: None,
            approval_context_digest: None,
            linked_at: None,
            termination: None,
        }
    }

    #[test]
    fn valid_row_converts() {
        let request = decode_request(
            row(),
            vec![ApprovalRow {
                approver_id: "20".to_string(),
                approved_at: Utc.with_ymd_and_hms(2026, 7, 12, 0, 1, 0).unwrap(),
                approval_payload_digest: None,
            }],
        )
        .unwrap();
        assert_eq!(request.id.as_str(), "request_1");
        assert_eq!(request.approvals[0].approver, UserId(20));
    }

    #[test]
    fn invalid_identity_version_hash_and_state_are_backend() {
        let mut invalid = row();
        invalid.id = "bad id".to_string();
        assert!(matches!(
            decode_request(invalid, vec![]),
            Err(ActivationStoreError::Backend(_))
        ));
        let mut invalid = row();
        invalid.target_version = 0;
        assert!(decode_request(invalid, vec![]).is_err());
        let mut invalid = row();
        invalid.target_content_hash = "no".to_string();
        assert!(decode_request(invalid, vec![]).is_err());
        let mut invalid = row();
        invalid.state = "unknown".to_string();
        assert!(decode_request(invalid, vec![]).is_err());
    }

    #[test]
    fn invalid_state_column_combinations_are_backend() {
        let mut invalid = row();
        invalid.state = "applying".to_string();
        assert!(decode_request(invalid, vec![]).is_err());
        let mut invalid = row();
        invalid.state = "applied".to_string();
        assert!(decode_request(invalid, vec![]).is_err());
        let mut invalid = row();
        invalid.state = "rejected".to_string();
        assert!(decode_request(invalid, vec![]).is_err());
    }
}
