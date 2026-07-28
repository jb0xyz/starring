use authoring_application::{
    AuthorizedApplyProductV1, AuthorizedApproveProductV1, AuthorizedCancelProductLifecycleV1,
    AuthorizedRejectProductV1,
};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;

use crate::product_action_digest::{
    keyed_digest, product_action_keyring_coverage_identity_v1,
    product_action_session_subject_digest_v1, unkeyed_digest, ProductActionDigestKeyringV1,
    ProductActionKeyringCoverageIdentityV1,
};

const IDEMPOTENCY_DOMAIN: &[u8] = b"starring.product.approval.idempotency.v1";
const SEMANTIC_REQUEST_DOMAIN: &[u8] = b"starring.product.approval.request.v1";
const RECEIPT_ID_DOMAIN: &[u8] = b"starring.product.approval.receipt.v1";
const AUDIT_EVENT_ID_DOMAIN: &[u8] = b"starring.product.approval.audit.v1";
const SESSION_SUBJECT_DOMAIN: &[u8] = b"starring.product.session.subject.v1";
const KEY_MATERIAL_FINGERPRINT_DOMAIN: &[u8] =
    b"starring.product.approval.digest-key-fingerprint.v1";
const APPLY_IDEMPOTENCY_DOMAIN: &[u8] = b"starring.product.apply.idempotency.v1";
const APPLY_SEMANTIC_REQUEST_DOMAIN: &[u8] = b"starring.product.apply.request.v1";
const APPLY_RECEIPT_ID_DOMAIN: &[u8] = b"starring.product.apply.receipt.v1";
const APPLY_AUDIT_EVENT_ID_DOMAIN: &[u8] = b"starring.product.apply.audit.v1";
const APPLY_ATTEMPT_ID_DOMAIN: &[u8] = b"starring.product.apply.attempt.v1";
const APPLY_DEPLOYMENT_ID_DOMAIN: &[u8] = b"starring.product.apply.deployment.v1";
const APPLY_DRAIN_CONSUME_TERMINAL_ACTION_ID_DOMAIN: &[u8] =
    b"starring.product.apply.runtime-drain-consume-terminal-action.v1";
const REJECTION_IDEMPOTENCY_DOMAIN: &[u8] = b"starring.product.rejection.idempotency.v1";
const REJECTION_SEMANTIC_REQUEST_DOMAIN: &[u8] = b"starring.product.rejection.request.v1";
const REJECTION_RECEIPT_ID_DOMAIN: &[u8] = b"starring.product.rejection.receipt.v1";
const REJECTION_AUDIT_EVENT_ID_DOMAIN: &[u8] = b"starring.product.rejection.audit.v1";
const REJECTION_KEY_MATERIAL_FINGERPRINT_DOMAIN: &[u8] =
    b"starring.product.rejection.digest-key-fingerprint.v1";
const LIFECYCLE_CANCELLATION_IDEMPOTENCY_DOMAIN: &[u8] =
    b"starring.product.lifecycle-cancellation.idempotency.v1";
const LIFECYCLE_CANCELLATION_SEMANTIC_REQUEST_DOMAIN: &[u8] =
    b"starring.product.lifecycle-cancellation.request.v1";
const LIFECYCLE_CANCELLATION_RECEIPT_ID_DOMAIN: &[u8] =
    b"starring.product.lifecycle-cancellation.receipt.v1";
const LIFECYCLE_CANCELLATION_AUDIT_EVENT_ID_DOMAIN: &[u8] =
    b"starring.product.lifecycle-cancellation.audit.v1";
const LIFECYCLE_CANCELLATION_TERMINAL_ACTION_ID_DOMAIN: &[u8] =
    b"starring.product.lifecycle-cancellation.terminal-action.v1";
const LIFECYCLE_CANCELLATION_REASON_DIGEST_DOMAIN: &[u8] =
    b"starring.product.lifecycle-cancellation.reason.v1";
const LIFECYCLE_CANCELLATION_SESSION_SUBJECT_DOMAIN: &[u8] =
    b"starring.product.lifecycle-cancellation.session-subject.v1";
const LIFECYCLE_CANCELLATION_KEY_MATERIAL_FINGERPRINT_DOMAIN: &[u8] =
    b"starring.product.lifecycle-cancellation.digest-key-fingerprint.v1";

pub(crate) struct ApprovalDigests {
    pub active_idempotency: String,
    pub idempotency_candidates: Vec<String>,
    pub idempotency_candidate_key_ids: Vec<String>,
    pub idempotency_candidate_key_fingerprints: Vec<String>,
    pub active_key_id: String,
    pub semantic_request: String,
    pub receipt_id: String,
    pub audit_event_id: String,
    pub session_subject: Vec<u8>,
}

pub(crate) struct ApplyDigests {
    pub active_idempotency: String,
    pub idempotency_candidates: Vec<String>,
    pub idempotency_candidate_key_ids: Vec<String>,
    pub idempotency_candidate_key_fingerprints: Vec<String>,
    pub active_key_id: String,
    pub semantic_request: String,
    pub receipt_id: String,
    pub audit_event_id: String,
    pub apply_attempt_id: String,
    pub deployment_id: String,
    pub drain_consume_terminal_action_id: String,
    pub session_subject: Vec<u8>,
}

pub(crate) struct RejectionDigests {
    pub active_idempotency: String,
    pub idempotency_candidates: Vec<String>,
    pub idempotency_candidate_key_ids: Vec<String>,
    pub idempotency_candidate_key_fingerprints: Vec<String>,
    pub active_key_id: String,
    pub semantic_request: String,
    pub receipt_id: String,
    pub audit_event_id: String,
    pub session_subject: Vec<u8>,
}

pub(crate) struct LifecycleCancellationDigests {
    pub active_idempotency: String,
    pub idempotency_candidates: Vec<String>,
    pub idempotency_candidate_key_ids: Vec<String>,
    pub idempotency_candidate_key_fingerprints: Vec<String>,
    pub active_key_id: String,
    pub semantic_request: String,
    pub receipt_id: String,
    pub audit_event_id: String,
    pub action_evidence_candidates: Vec<(String, String)>,
    pub terminal_action_id: String,
    pub reason_digest: String,
    pub session_subject: Vec<u8>,
}

struct RejectionDigestMaterial<'a> {
    tenant_id: &'a str,
    installation_id: &'a str,
    principal_id: &'a str,
    promotion_id: &'a str,
    expected_revision: String,
    expected_payload_digest: &'a str,
    idempotency_key: &'a str,
    reason: &'a str,
    session_fingerprint: &'a [u8],
}

struct LifecycleCancellationDigestMaterial<'a> {
    tenant_id: &'a str,
    installation_id: &'a str,
    principal_id: &'a str,
    promotion_id: &'a str,
    expected_product_revision: String,
    expected_payload_digest: &'a str,
    drain_intent_id: &'a str,
    acknowledged_intent_revision: String,
    acknowledged_state_digest: &'a str,
    product_operation_id: &'a str,
    expected_runtime_deployment_revision: String,
    idempotency_key: &'a str,
    reason: &'a str,
    session_fingerprint: &'a [u8],
}

pub(crate) fn keyring_coverage_identity(
    keyring: &ProductActionDigestKeyringV1,
) -> ProductActionKeyringCoverageIdentityV1 {
    product_action_keyring_coverage_identity_v1(keyring, KEY_MATERIAL_FINGERPRINT_DOMAIN)
}

pub(crate) fn approval_digests(
    keyring: &ProductActionDigestKeyringV1,
    request: &AuthorizedApproveProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
) -> ApprovalDigests {
    let scope = request.scope();
    let command = request.command();
    let expected_revision = command.expected_revision.get().to_string();
    let idempotency_fields = [
        scope.tenant_id().as_str().as_bytes(),
        scope.installation_id().as_str().as_bytes(),
        request.actor().principal_id().as_str().as_bytes(),
        b"product_approve_v1".as_slice(),
        command.idempotency_key.as_str().as_bytes(),
    ];
    let idempotency_candidates = keyring
        .keys()
        .iter()
        .map(|key| keyed_digest(key, IDEMPOTENCY_DOMAIN, &idempotency_fields))
        .collect::<Vec<_>>();
    let active_idempotency = idempotency_candidates[0].clone();
    let keyring_identity = keyring_coverage_identity(keyring);
    let semantic_request = unkeyed_digest(
        SEMANTIC_REQUEST_DOMAIN,
        &[
            scope.tenant_id().as_str().as_bytes(),
            scope.installation_id().as_str().as_bytes(),
            request.actor().principal_id().as_str().as_bytes(),
            command.promotion.promotion_id().as_str().as_bytes(),
            expected_revision.as_bytes(),
            command.expected_payload_digest.as_str().as_bytes(),
        ],
    );
    let identity_fields = [
        scope.tenant_id().as_str().as_bytes(),
        scope.installation_id().as_str().as_bytes(),
        request.actor().principal_id().as_str().as_bytes(),
        active_idempotency.as_bytes(),
        semantic_request.as_bytes(),
    ];
    let receipt_id = keyed_digest(keyring.active(), RECEIPT_ID_DOMAIN, &identity_fields);
    let audit_event_id = keyed_digest(keyring.active(), AUDIT_EVENT_ID_DOMAIN, &identity_fields);
    let session_subject = product_action_session_subject_digest_v1(
        SESSION_SUBJECT_DOMAIN,
        scope.tenant_id().as_str().as_bytes(),
        request.actor().principal_id().as_str().as_bytes(),
        request.session_fingerprint().as_bytes().as_slice(),
    );
    ApprovalDigests {
        active_idempotency,
        idempotency_candidates,
        idempotency_candidate_key_ids: keyring_identity.key_ids,
        idempotency_candidate_key_fingerprints: keyring_identity.key_fingerprints,
        active_key_id: keyring.active().key_id().to_string(),
        semantic_request,
        receipt_id,
        audit_event_id,
        session_subject,
    }
}

pub(crate) fn apply_digests(
    keyring: &ProductActionDigestKeyringV1,
    request: &AuthorizedApplyProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
) -> ApplyDigests {
    let scope = request.scope();
    let command = request.command();
    let expected_revision = command.expected_revision.get().to_string();
    let idempotency_fields = [
        scope.tenant_id().as_str().as_bytes(),
        scope.installation_id().as_str().as_bytes(),
        request.actor().principal_id().as_str().as_bytes(),
        b"product_apply_v1".as_slice(),
        command.idempotency_key.as_str().as_bytes(),
    ];
    let idempotency_candidates = keyring
        .keys()
        .iter()
        .map(|key| keyed_digest(key, APPLY_IDEMPOTENCY_DOMAIN, &idempotency_fields))
        .collect::<Vec<_>>();
    let active_idempotency = idempotency_candidates[0].clone();
    let keyring_identity = keyring_coverage_identity(keyring);
    let semantic_request = unkeyed_digest(
        APPLY_SEMANTIC_REQUEST_DOMAIN,
        &[
            scope.tenant_id().as_str().as_bytes(),
            scope.installation_id().as_str().as_bytes(),
            request.actor().principal_id().as_str().as_bytes(),
            command.promotion.promotion_id().as_str().as_bytes(),
            expected_revision.as_bytes(),
            command.expected_payload_digest.as_str().as_bytes(),
        ],
    );
    let identity_fields = [
        scope.tenant_id().as_str().as_bytes(),
        scope.installation_id().as_str().as_bytes(),
        request.actor().principal_id().as_str().as_bytes(),
        command.promotion.promotion_id().as_str().as_bytes(),
        active_idempotency.as_bytes(),
        semantic_request.as_bytes(),
    ];
    let receipt_id = keyed_digest(keyring.active(), APPLY_RECEIPT_ID_DOMAIN, &identity_fields);
    let audit_event_id = keyed_digest(
        keyring.active(),
        APPLY_AUDIT_EVENT_ID_DOMAIN,
        &identity_fields,
    );
    let apply_attempt_id =
        keyed_digest(keyring.active(), APPLY_ATTEMPT_ID_DOMAIN, &identity_fields);
    let deployment_id = keyed_digest(
        keyring.active(),
        APPLY_DEPLOYMENT_ID_DOMAIN,
        &identity_fields,
    );
    let drain_consume_terminal_action_id = keyed_digest(
        keyring.active(),
        APPLY_DRAIN_CONSUME_TERMINAL_ACTION_ID_DOMAIN,
        &identity_fields,
    );
    let session_subject = product_action_session_subject_digest_v1(
        SESSION_SUBJECT_DOMAIN,
        scope.tenant_id().as_str().as_bytes(),
        request.actor().principal_id().as_str().as_bytes(),
        request.session_fingerprint().as_bytes().as_slice(),
    );
    ApplyDigests {
        active_idempotency,
        idempotency_candidates,
        idempotency_candidate_key_ids: keyring_identity.key_ids,
        idempotency_candidate_key_fingerprints: keyring_identity.key_fingerprints,
        active_key_id: keyring.active().key_id().to_string(),
        semantic_request,
        receipt_id,
        audit_event_id,
        apply_attempt_id,
        deployment_id,
        drain_consume_terminal_action_id,
        session_subject,
    }
}

pub(crate) fn rejection_digests(
    keyring: &ProductActionDigestKeyringV1,
    request: &AuthorizedRejectProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
) -> RejectionDigests {
    let scope = request.scope();
    let command = request.command();
    rejection_digests_from_material(
        keyring,
        RejectionDigestMaterial {
            tenant_id: scope.tenant_id().as_str(),
            installation_id: scope.installation_id().as_str(),
            principal_id: request.actor().principal_id().as_str(),
            promotion_id: command.promotion.promotion_id().as_str(),
            expected_revision: command.expected_revision.get().to_string(),
            expected_payload_digest: command.expected_payload_digest.as_str(),
            idempotency_key: command.idempotency_key.as_str(),
            reason: command.reason.as_str(),
            session_fingerprint: request.session_fingerprint().as_bytes().as_slice(),
        },
    )
}

pub(crate) fn lifecycle_cancellation_digests(
    keyring: &ProductActionDigestKeyringV1,
    request: &AuthorizedCancelProductLifecycleV1<'_, FreshDiscordAuthorityEvidenceV1>,
) -> LifecycleCancellationDigests {
    let scope = request.scope();
    let command = request.command();
    let selector = &command.drain_selector;
    lifecycle_cancellation_digests_from_material(
        keyring,
        LifecycleCancellationDigestMaterial {
            tenant_id: scope.tenant_id().as_str(),
            installation_id: scope.installation_id().as_str(),
            principal_id: request.actor().principal_id().as_str(),
            promotion_id: command.promotion.promotion_id().as_str(),
            expected_product_revision: command.expected_revision.get().to_string(),
            expected_payload_digest: command.expected_payload_digest.as_str(),
            drain_intent_id: selector.drain_intent_id(),
            acknowledged_intent_revision: selector.acknowledged_intent_revision().get().to_string(),
            acknowledged_state_digest: selector.acknowledged_state_digest(),
            product_operation_id: selector.product_operation_id(),
            expected_runtime_deployment_revision: selector
                .expected_runtime_deployment_revision()
                .get()
                .to_string(),
            idempotency_key: command.idempotency_key.as_str(),
            reason: command.reason.as_str(),
            session_fingerprint: request.session_fingerprint().as_bytes().as_slice(),
        },
    )
}

fn lifecycle_cancellation_digests_from_material(
    keyring: &ProductActionDigestKeyringV1,
    material: LifecycleCancellationDigestMaterial<'_>,
) -> LifecycleCancellationDigests {
    let idempotency_fields = [
        material.tenant_id.as_bytes(),
        material.installation_id.as_bytes(),
        material.principal_id.as_bytes(),
        b"product_cancel_lifecycle_v1".as_slice(),
        material.idempotency_key.as_bytes(),
    ];
    let idempotency_candidates = keyring
        .keys()
        .iter()
        .map(|key| {
            keyed_digest(
                key,
                LIFECYCLE_CANCELLATION_IDEMPOTENCY_DOMAIN,
                &idempotency_fields,
            )
        })
        .collect::<Vec<_>>();
    let active_idempotency = idempotency_candidates[0].clone();
    let keyring_identity = product_action_keyring_coverage_identity_v1(
        keyring,
        LIFECYCLE_CANCELLATION_KEY_MATERIAL_FINGERPRINT_DOMAIN,
    );
    let semantic_request = unkeyed_digest(
        LIFECYCLE_CANCELLATION_SEMANTIC_REQUEST_DOMAIN,
        &[
            material.tenant_id.as_bytes(),
            material.installation_id.as_bytes(),
            material.principal_id.as_bytes(),
            material.promotion_id.as_bytes(),
            material.expected_product_revision.as_bytes(),
            material.expected_payload_digest.as_bytes(),
            material.drain_intent_id.as_bytes(),
            material.acknowledged_intent_revision.as_bytes(),
            material.acknowledged_state_digest.as_bytes(),
            material.product_operation_id.as_bytes(),
            material.expected_runtime_deployment_revision.as_bytes(),
            material.reason.as_bytes(),
        ],
    );
    let action_evidence_candidates = keyring
        .keys()
        .iter()
        .zip(idempotency_candidates.iter())
        .map(|(key, idempotency)| {
            let identity_fields = [
                material.tenant_id.as_bytes(),
                material.installation_id.as_bytes(),
                material.principal_id.as_bytes(),
                material.promotion_id.as_bytes(),
                idempotency.as_bytes(),
                semantic_request.as_bytes(),
            ];
            (
                keyed_digest(
                    key,
                    LIFECYCLE_CANCELLATION_RECEIPT_ID_DOMAIN,
                    &identity_fields,
                ),
                keyed_digest(
                    key,
                    LIFECYCLE_CANCELLATION_AUDIT_EVENT_ID_DOMAIN,
                    &identity_fields,
                ),
            )
        })
        .collect::<Vec<_>>();
    let (receipt_id, audit_event_id) = action_evidence_candidates[0].clone();
    let terminal_action_id = unkeyed_digest(
        LIFECYCLE_CANCELLATION_TERMINAL_ACTION_ID_DOMAIN,
        &[semantic_request.as_bytes()],
    );
    let reason_digest = unkeyed_digest(
        LIFECYCLE_CANCELLATION_REASON_DIGEST_DOMAIN,
        &[
            material.tenant_id.as_bytes(),
            material.installation_id.as_bytes(),
            material.promotion_id.as_bytes(),
            material.drain_intent_id.as_bytes(),
            material.reason.as_bytes(),
        ],
    );
    let session_subject = product_action_session_subject_digest_v1(
        LIFECYCLE_CANCELLATION_SESSION_SUBJECT_DOMAIN,
        material.tenant_id.as_bytes(),
        material.principal_id.as_bytes(),
        material.session_fingerprint,
    );
    LifecycleCancellationDigests {
        active_idempotency,
        idempotency_candidates,
        idempotency_candidate_key_ids: keyring_identity.key_ids,
        idempotency_candidate_key_fingerprints: keyring_identity.key_fingerprints,
        active_key_id: keyring.active().key_id().to_string(),
        semantic_request,
        receipt_id,
        audit_event_id,
        action_evidence_candidates,
        terminal_action_id,
        reason_digest,
        session_subject,
    }
}

fn rejection_digests_from_material(
    keyring: &ProductActionDigestKeyringV1,
    material: RejectionDigestMaterial<'_>,
) -> RejectionDigests {
    let idempotency_fields = [
        material.tenant_id.as_bytes(),
        material.installation_id.as_bytes(),
        material.principal_id.as_bytes(),
        b"product_reject_v1".as_slice(),
        material.idempotency_key.as_bytes(),
    ];
    let idempotency_candidates = keyring
        .keys()
        .iter()
        .map(|key| keyed_digest(key, REJECTION_IDEMPOTENCY_DOMAIN, &idempotency_fields))
        .collect::<Vec<_>>();
    let active_idempotency = idempotency_candidates[0].clone();
    let keyring_identity = product_action_keyring_coverage_identity_v1(
        keyring,
        REJECTION_KEY_MATERIAL_FINGERPRINT_DOMAIN,
    );
    let semantic_request = unkeyed_digest(
        REJECTION_SEMANTIC_REQUEST_DOMAIN,
        &[
            material.tenant_id.as_bytes(),
            material.installation_id.as_bytes(),
            material.principal_id.as_bytes(),
            material.promotion_id.as_bytes(),
            material.expected_revision.as_bytes(),
            material.expected_payload_digest.as_bytes(),
            material.reason.as_bytes(),
        ],
    );
    let identity_fields = [
        material.tenant_id.as_bytes(),
        material.installation_id.as_bytes(),
        material.principal_id.as_bytes(),
        active_idempotency.as_bytes(),
        semantic_request.as_bytes(),
    ];
    let receipt_id = keyed_digest(
        keyring.active(),
        REJECTION_RECEIPT_ID_DOMAIN,
        &identity_fields,
    );
    let audit_event_id = keyed_digest(
        keyring.active(),
        REJECTION_AUDIT_EVENT_ID_DOMAIN,
        &identity_fields,
    );
    let session_subject = product_action_session_subject_digest_v1(
        SESSION_SUBJECT_DOMAIN,
        material.tenant_id.as_bytes(),
        material.principal_id.as_bytes(),
        material.session_fingerprint,
    );
    RejectionDigests {
        active_idempotency,
        idempotency_candidates,
        idempotency_candidate_key_ids: keyring_identity.key_ids,
        idempotency_candidate_key_fingerprints: keyring_identity.key_fingerprints,
        active_key_id: keyring.active().key_id().to_string(),
        semantic_request,
        receipt_id,
        audit_event_id,
        session_subject,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::product_action_digest::{unkeyed_digest_bytes, ProductActionDigestKeyV1};

    const TEST_PROMOTION_ID: &str =
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const TEST_DRAIN_INTENT_ID: &str = "11111111111111111111111111111111";
    const TEST_ALTERNATE_DRAIN_INTENT_ID: &str = "33333333333333333333333333333333";
    const TEST_ACKNOWLEDGED_STATE_DIGEST: &str =
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const TEST_PRODUCT_OPERATION_ID: &str = "22222222222222222222222222222222";
    const TEST_PAYLOAD_DIGEST: &str =
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    fn cancellation_keyring() -> ProductActionDigestKeyringV1 {
        ProductActionDigestKeyringV1::new(
            ProductActionDigestKeyV1::from_bytes(
                "cancel-active",
                std::array::from_fn(|index| 71_u8.wrapping_add(index as u8)),
            )
            .unwrap(),
            [ProductActionDigestKeyV1::from_bytes(
                "cancel-retired",
                std::array::from_fn(|index| 137_u8.wrapping_add(index as u8)),
            )
            .unwrap()],
        )
        .unwrap()
    }

    fn cancellation_digests_for(
        keyring: &ProductActionDigestKeyringV1,
        reason: &str,
        drain_intent_id: &str,
        expected_product_revision: u64,
    ) -> LifecycleCancellationDigests {
        lifecycle_cancellation_digests_from_material(
            keyring,
            LifecycleCancellationDigestMaterial {
                tenant_id: "tenant",
                installation_id: "installation",
                principal_id: "principal",
                promotion_id: TEST_PROMOTION_ID,
                expected_product_revision: expected_product_revision.to_string(),
                expected_payload_digest: TEST_PAYLOAD_DIGEST,
                drain_intent_id,
                acknowledged_intent_revision: "7".to_string(),
                acknowledged_state_digest: TEST_ACKNOWLEDGED_STATE_DIGEST,
                product_operation_id: TEST_PRODUCT_OPERATION_ID,
                expected_runtime_deployment_revision: "10".to_string(),
                idempotency_key: "same-cancellation-key",
                reason,
                session_fingerprint: &[29_u8; 32],
            },
        )
    }

    #[test]
    fn hmac_is_domain_separated_and_key_bound() {
        let first =
            ProductActionDigestKeyV1::from_bytes("v1", std::array::from_fn(|index| index as u8))
                .unwrap();
        let second = ProductActionDigestKeyV1::from_bytes(
            "v2",
            std::array::from_fn(|index| 255_u8.wrapping_sub(index as u8)),
        )
        .unwrap();
        let fields = [b"tenant".as_slice(), b"same-low-entropy-key".as_slice()];
        assert_eq!(
            keyed_digest(&first, IDEMPOTENCY_DOMAIN, &fields),
            "bd00aaead854aea123bcb9021f835f7f7b26698d3bed0faf8a28645dca12d705"
        );
        assert_ne!(
            keyed_digest(&first, IDEMPOTENCY_DOMAIN, &fields),
            keyed_digest(&second, IDEMPOTENCY_DOMAIN, &fields)
        );
        assert_ne!(
            keyed_digest(&first, IDEMPOTENCY_DOMAIN, &fields),
            keyed_digest(&first, RECEIPT_ID_DOMAIN, &fields)
        );
        assert_ne!(
            keyed_digest(&first, IDEMPOTENCY_DOMAIN, &fields),
            keyed_digest(&first, SESSION_SUBJECT_DOMAIN, &fields)
        );
    }

    #[test]
    fn length_framing_distinguishes_ambiguous_field_boundaries() {
        assert_eq!(
            unkeyed_digest(SEMANTIC_REQUEST_DOMAIN, &[b"a", b"bc"]),
            "340f29cac7e35289d9ff44dcaefe36c87fb698d562863e334bd99b8e125e0f92"
        );
        assert_ne!(
            unkeyed_digest(SEMANTIC_REQUEST_DOMAIN, &[b"a", b"bc"]),
            unkeyed_digest(SEMANTIC_REQUEST_DOMAIN, &[b"ab", b"c"])
        );
    }

    #[test]
    fn session_subject_is_stable_opaque_and_domain_separated() {
        let session = [23_u8; 32];
        let fields = [
            b"tenant".as_slice(),
            b"principal".as_slice(),
            session.as_slice(),
        ];
        let first = unkeyed_digest_bytes(SESSION_SUBJECT_DOMAIN, &fields);
        let second = unkeyed_digest_bytes(SESSION_SUBJECT_DOMAIN, &fields);
        assert_eq!(first, second);
        assert_eq!(first.len(), 32);
        assert_ne!(first, session);
        assert_ne!(
            first,
            unkeyed_digest_bytes(SEMANTIC_REQUEST_DOMAIN, &fields)
        );
    }

    #[test]
    fn key_material_fingerprint_changes_when_an_identifier_is_reused() {
        let first = ProductActionDigestKeyV1::from_bytes(
            "reused",
            std::array::from_fn(|index| 17_u8.wrapping_add(index as u8)),
        )
        .unwrap();
        let second = ProductActionDigestKeyV1::from_bytes(
            "reused",
            std::array::from_fn(|index| 113_u8.wrapping_add(index as u8)),
        )
        .unwrap();
        assert_ne!(
            unkeyed_digest(KEY_MATERIAL_FINGERPRINT_DOMAIN, &[first.secret()]),
            unkeyed_digest(KEY_MATERIAL_FINGERPRINT_DOMAIN, &[second.secret()])
        );
    }

    #[test]
    fn keyring_coverage_identity_preserves_secret_order_without_secret_material() {
        let first = ProductActionDigestKeyV1::from_bytes(
            "first",
            std::array::from_fn(|index| 31_u8.wrapping_add(index as u8)),
        )
        .unwrap();
        let second = ProductActionDigestKeyV1::from_bytes(
            "second",
            std::array::from_fn(|index| 97_u8.wrapping_add(index as u8)),
        )
        .unwrap();
        let keyring = ProductActionDigestKeyringV1::new(first, [second]).unwrap();
        let identity = keyring_coverage_identity(&keyring);
        assert_eq!(identity.key_ids, ["first", "second"]);
        assert_eq!(identity.key_fingerprints.len(), 2);
        assert!(identity
            .key_fingerprints
            .iter()
            .all(|fingerprint| fingerprint.len() == 64));
        assert!(!format!("{:?}", keyring).contains(&identity.key_fingerprints[0]));
    }

    #[test]
    fn rejection_reason_changes_only_semantic_identity_inputs() {
        let keyring = ProductActionDigestKeyringV1::new(
            ProductActionDigestKeyV1::from_bytes(
                "active",
                std::array::from_fn(|index| 41_u8.wrapping_add(index as u8)),
            )
            .unwrap(),
            [],
        )
        .unwrap();
        let build = |reason| {
            rejection_digests_from_material(
                &keyring,
                RejectionDigestMaterial {
                    tenant_id: "tenant",
                    installation_id: "installation",
                    principal_id: "principal",
                    promotion_id: &"a".repeat(64),
                    expected_revision: "7".to_string(),
                    expected_payload_digest: &"b".repeat(64),
                    idempotency_key: "same-key",
                    reason,
                    session_fingerprint: &[19_u8; 32],
                },
            )
        };
        let first = build("unsafe permission scope");
        let second = build("unexpected channel visibility");
        assert_eq!(first.active_idempotency, second.active_idempotency);
        assert_eq!(first.idempotency_candidates, second.idempotency_candidates);
        assert_eq!(first.session_subject, second.session_subject);
        assert_ne!(first.semantic_request, second.semantic_request);
        assert_ne!(first.receipt_id, second.receipt_id);
        assert_ne!(first.audit_event_id, second.audit_event_id);
    }

    #[test]
    fn rejection_digest_domains_are_separate_from_approval() {
        let key = ProductActionDigestKeyV1::from_bytes(
            "active",
            std::array::from_fn(|index| 59_u8.wrapping_add(index as u8)),
        )
        .unwrap();
        let fields = [b"same".as_slice(), b"material".as_slice()];
        assert_ne!(
            keyed_digest(&key, IDEMPOTENCY_DOMAIN, &fields),
            keyed_digest(&key, REJECTION_IDEMPOTENCY_DOMAIN, &fields)
        );
        assert_ne!(
            unkeyed_digest(SEMANTIC_REQUEST_DOMAIN, &fields),
            unkeyed_digest(REJECTION_SEMANTIC_REQUEST_DOMAIN, &fields)
        );
        assert_ne!(
            keyed_digest(&key, RECEIPT_ID_DOMAIN, &fields),
            keyed_digest(&key, REJECTION_RECEIPT_ID_DOMAIN, &fields)
        );
        assert_ne!(
            keyed_digest(&key, AUDIT_EVENT_ID_DOMAIN, &fields),
            keyed_digest(&key, REJECTION_AUDIT_EVENT_ID_DOMAIN, &fields)
        );
        assert_ne!(
            unkeyed_digest(KEY_MATERIAL_FINGERPRINT_DOMAIN, &fields),
            unkeyed_digest(REJECTION_KEY_MATERIAL_FINGERPRINT_DOMAIN, &fields)
        );
    }

    #[test]
    fn lifecycle_cancellation_domains_are_distinct_and_length_framed() {
        let keyring = cancellation_keyring();
        let key = keyring.active();
        let fields = [b"same".as_slice(), b"material".as_slice()];
        for domain in [
            APPLY_IDEMPOTENCY_DOMAIN,
            APPLY_RECEIPT_ID_DOMAIN,
            APPLY_AUDIT_EVENT_ID_DOMAIN,
            APPLY_ATTEMPT_ID_DOMAIN,
            APPLY_DEPLOYMENT_ID_DOMAIN,
        ] {
            assert_ne!(
                keyed_digest(key, APPLY_DRAIN_CONSUME_TERMINAL_ACTION_ID_DOMAIN, &fields),
                keyed_digest(key, domain, &fields)
            );
        }
        for domain in [
            LIFECYCLE_CANCELLATION_IDEMPOTENCY_DOMAIN,
            LIFECYCLE_CANCELLATION_RECEIPT_ID_DOMAIN,
            LIFECYCLE_CANCELLATION_AUDIT_EVENT_ID_DOMAIN,
            LIFECYCLE_CANCELLATION_TERMINAL_ACTION_ID_DOMAIN,
        ] {
            assert_ne!(
                keyed_digest(key, domain, &fields),
                keyed_digest(key, APPLY_IDEMPOTENCY_DOMAIN, &fields)
            );
            assert_ne!(
                keyed_digest(key, domain, &fields),
                keyed_digest(key, REJECTION_IDEMPOTENCY_DOMAIN, &fields)
            );
            assert_ne!(
                keyed_digest(key, domain, &fields),
                keyed_digest(key, APPLY_DRAIN_CONSUME_TERMINAL_ACTION_ID_DOMAIN, &fields)
            );
        }
        assert_ne!(
            unkeyed_digest(LIFECYCLE_CANCELLATION_SEMANTIC_REQUEST_DOMAIN, &fields),
            unkeyed_digest(APPLY_SEMANTIC_REQUEST_DOMAIN, &fields)
        );
        assert_ne!(
            unkeyed_digest(LIFECYCLE_CANCELLATION_REASON_DIGEST_DOMAIN, &fields),
            unkeyed_digest(LIFECYCLE_CANCELLATION_SEMANTIC_REQUEST_DOMAIN, &fields)
        );
        assert_ne!(
            unkeyed_digest_bytes(LIFECYCLE_CANCELLATION_SESSION_SUBJECT_DOMAIN, &fields),
            unkeyed_digest_bytes(SESSION_SUBJECT_DOMAIN, &fields)
        );
        assert_ne!(
            unkeyed_digest(
                LIFECYCLE_CANCELLATION_KEY_MATERIAL_FINGERPRINT_DOMAIN,
                &fields
            ),
            unkeyed_digest(KEY_MATERIAL_FINGERPRINT_DOMAIN, &fields)
        );
        assert_ne!(
            unkeyed_digest(
                LIFECYCLE_CANCELLATION_SEMANTIC_REQUEST_DOMAIN,
                &[b"a", b"bc"]
            ),
            unkeyed_digest(
                LIFECYCLE_CANCELLATION_SEMANTIC_REQUEST_DOMAIN,
                &[b"ab", b"c"]
            )
        );
    }

    #[test]
    fn lifecycle_cancellation_binds_selector_reason_revision_and_key_rotation() {
        let keyring = cancellation_keyring();
        let first = cancellation_digests_for(
            &keyring,
            "retain the existing deployment",
            TEST_DRAIN_INTENT_ID,
            3,
        );
        let reason_changed = cancellation_digests_for(
            &keyring,
            "release the blocked product lifecycle",
            TEST_DRAIN_INTENT_ID,
            3,
        );
        let selector_changed = cancellation_digests_for(
            &keyring,
            "retain the existing deployment",
            TEST_ALTERNATE_DRAIN_INTENT_ID,
            3,
        );
        let revision_changed = cancellation_digests_for(
            &keyring,
            "retain the existing deployment",
            TEST_DRAIN_INTENT_ID,
            4,
        );
        for changed in [&reason_changed, &selector_changed, &revision_changed] {
            assert_eq!(first.active_idempotency, changed.active_idempotency);
            assert_eq!(first.idempotency_candidates, changed.idempotency_candidates);
            assert_eq!(
                first.idempotency_candidate_key_ids,
                changed.idempotency_candidate_key_ids
            );
            assert_eq!(
                first.idempotency_candidate_key_fingerprints,
                changed.idempotency_candidate_key_fingerprints
            );
            assert_eq!(first.session_subject, changed.session_subject);
            assert_ne!(first.semantic_request, changed.semantic_request);
            assert_ne!(first.receipt_id, changed.receipt_id);
            assert_ne!(first.audit_event_id, changed.audit_event_id);
            assert_ne!(first.terminal_action_id, changed.terminal_action_id);
        }
        assert_ne!(first.reason_digest, reason_changed.reason_digest);
        assert_ne!(first.reason_digest, selector_changed.reason_digest);
        assert_eq!(first.reason_digest, revision_changed.reason_digest);
        assert_eq!(first.active_key_id, "cancel-active");
        assert_eq!(
            first.idempotency_candidate_key_ids,
            ["cancel-active", "cancel-retired"]
        );
        assert_eq!(first.idempotency_candidates.len(), 2);
        assert_eq!(first.idempotency_candidate_key_fingerprints.len(), 2);
        assert_eq!(first.active_idempotency, first.idempotency_candidates[0]);
        for digest in [
            &first.active_idempotency,
            &first.semantic_request,
            &first.receipt_id,
            &first.audit_event_id,
            &first.terminal_action_id,
            &first.reason_digest,
        ] {
            assert_eq!(digest.len(), 64);
            assert!(digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
        }
        assert_eq!(first.session_subject.len(), 32);
    }

    #[test]
    fn lifecycle_cancellation_terminal_identity_survives_key_rotation() {
        let original = cancellation_keyring();
        let rotated = ProductActionDigestKeyringV1::new(
            ProductActionDigestKeyV1::from_bytes(
                "cancel-next",
                std::array::from_fn(|index| 193_u8.wrapping_add(index as u8)),
            )
            .unwrap(),
            [
                ProductActionDigestKeyV1::from_bytes(
                    "cancel-active",
                    std::array::from_fn(|index| 71_u8.wrapping_add(index as u8)),
                )
                .unwrap(),
                ProductActionDigestKeyV1::from_bytes(
                    "cancel-retired",
                    std::array::from_fn(|index| 137_u8.wrapping_add(index as u8)),
                )
                .unwrap(),
            ],
        )
        .unwrap();
        let first = cancellation_digests_for(
            &original,
            "retain the existing deployment",
            TEST_DRAIN_INTENT_ID,
            3,
        );
        let replay = cancellation_digests_for(
            &rotated,
            "retain the existing deployment",
            TEST_DRAIN_INTENT_ID,
            3,
        );
        assert_eq!(first.semantic_request, replay.semantic_request);
        assert_eq!(first.terminal_action_id, replay.terminal_action_id);
        assert_ne!(first.active_idempotency, replay.active_idempotency);
        assert_ne!(first.receipt_id, replay.receipt_id);
        assert_ne!(first.audit_event_id, replay.audit_event_id);
        assert!(replay
            .idempotency_candidates
            .iter()
            .any(|candidate| candidate == &first.active_idempotency));
        assert!(replay
            .action_evidence_candidates
            .iter()
            .any(
                |candidate| candidate.0 == first.receipt_id && candidate.1 == first.audit_event_id
            ));
    }
}
