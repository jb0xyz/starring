use std::fmt::{Debug, Formatter};

use authoring_application::{
    AuthoringExpectedGenerationV1, AuthoringHumanMessageV1, ProductIdempotencyKeyV1,
};
use authoring_promotion::{AuthoringSessionId, AutomationInstallationId, PrincipalId, TenantId};

use crate::product_action_digest::{keyed_digest, unkeyed_digest};
use crate::{ProductActionDigestKeyV1, ProductActionDigestKeyringV1};

const WRITER_REQUEST_DOMAIN_V1: &[u8] = b"starring.authoring.writer_request.v1";
const WRITER_SEMANTIC_DOMAIN_V1: &[u8] = b"starring.authoring.writer_semantic_request.v1";
const WRITER_KEY_FINGERPRINT_DOMAIN_V1: &[u8] =
    b"starring.authoring.writer_digest_key_fingerprint.v1";
const SAFE_PROJECTION_DOMAIN_V1: &[u8] = b"starring.authoring.safe_turn_projection.v1";

pub(super) struct WriterDigestInputV1<'a> {
    pub tenant_id: &'a TenantId,
    pub installation_id: &'a AutomationInstallationId,
    pub principal_id: &'a PrincipalId,
    pub session_id: &'a AuthoringSessionId,
    pub expected_generation: AuthoringExpectedGenerationV1,
    pub idempotency_key: &'a ProductIdempotencyKeyV1,
    pub human_message: &'a AuthoringHumanMessageV1,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct WriterDigestCandidateV1 {
    request_digest: String,
    semantic_digest: String,
    key_id: String,
    key_fingerprint: String,
}

impl WriterDigestCandidateV1 {
    pub fn request_digest(&self) -> &str {
        &self.request_digest
    }

    pub fn semantic_digest(&self) -> &str {
        &self.semantic_digest
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    pub fn key_fingerprint(&self) -> &str {
        &self.key_fingerprint
    }
}

impl Debug for WriterDigestCandidateV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("WriterDigestCandidateV1(<redacted>)")
    }
}

pub(super) fn writer_digest_candidates_v1(
    keyring: &ProductActionDigestKeyringV1,
    input: WriterDigestInputV1<'_>,
) -> Vec<WriterDigestCandidateV1> {
    keyring
        .keys()
        .iter()
        .map(|key| writer_digest_candidate_v1(key, &input))
        .collect()
}

fn writer_digest_candidate_v1(
    key: &ProductActionDigestKeyV1,
    input: &WriterDigestInputV1<'_>,
) -> WriterDigestCandidateV1 {
    let expected_generation = input.expected_generation.get().to_be_bytes();
    WriterDigestCandidateV1 {
        request_digest: keyed_digest(
            key,
            WRITER_REQUEST_DOMAIN_V1,
            &[
                input.tenant_id.as_str().as_bytes(),
                input.installation_id.as_str().as_bytes(),
                input.principal_id.as_str().as_bytes(),
                input.session_id.as_str().as_bytes(),
                input.idempotency_key.as_str().as_bytes(),
            ],
        ),
        semantic_digest: keyed_digest(
            key,
            WRITER_SEMANTIC_DOMAIN_V1,
            &[
                expected_generation.as_slice(),
                input.human_message.as_str().as_bytes(),
            ],
        ),
        key_id: key.key_id().to_string(),
        key_fingerprint: unkeyed_digest(
            WRITER_KEY_FINGERPRINT_DOMAIN_V1,
            &[key.secret().as_slice()],
        ),
    }
}

pub(super) fn safe_projection_digest_v1(canonical_projection: &[u8]) -> String {
    unkeyed_digest(SAFE_PROJECTION_DOMAIN_V1, &[canonical_projection])
}

pub(super) struct WriterKeyringCoverageIdentityV1 {
    pub key_ids: Vec<String>,
    pub key_fingerprints: Vec<String>,
}

pub(super) fn writer_keyring_coverage_identity_v1(
    keyring: &ProductActionDigestKeyringV1,
) -> WriterKeyringCoverageIdentityV1 {
    let candidates = keyring
        .keys()
        .iter()
        .map(|key| {
            (
                key.key_id().to_string(),
                unkeyed_digest(WRITER_KEY_FINGERPRINT_DOMAIN_V1, &[key.secret().as_slice()]),
            )
        })
        .collect::<Vec<_>>();
    WriterKeyringCoverageIdentityV1 {
        key_ids: candidates
            .iter()
            .map(|candidate| candidate.0.clone())
            .collect(),
        key_fingerprints: candidates
            .into_iter()
            .map(|candidate| candidate.1)
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProductActionDigestKeyV1, ProductActionDigestKeyringV1};

    fn key(id: &str, seed: u8) -> ProductActionDigestKeyV1 {
        ProductActionDigestKeyV1::from_bytes(
            id,
            std::array::from_fn(|index| seed.wrapping_add(index as u8)),
        )
        .unwrap()
    }

    fn input<'a>(
        tenant_id: &'a TenantId,
        installation_id: &'a AutomationInstallationId,
        principal_id: &'a PrincipalId,
        session_id: &'a AuthoringSessionId,
        idempotency_key: &'a ProductIdempotencyKeyV1,
        human_message: &'a AuthoringHumanMessageV1,
    ) -> WriterDigestInputV1<'a> {
        WriterDigestInputV1 {
            tenant_id,
            installation_id,
            principal_id,
            session_id,
            expected_generation: AuthoringExpectedGenerationV1::new(7).unwrap(),
            idempotency_key,
            human_message,
        }
    }

    #[test]
    fn active_and_retired_candidates_bind_scope_key_generation_and_message() {
        let tenant_id = TenantId::parse("tenant-1").unwrap();
        let installation_id = AutomationInstallationId::parse("installation-1").unwrap();
        let principal_id = PrincipalId::parse("principal-1").unwrap();
        let session_id = AuthoringSessionId::parse("session-1").unwrap();
        let idempotency_key = ProductIdempotencyKeyV1::parse("request-1").unwrap();
        let human_message = AuthoringHumanMessageV1::parse("Create a room").unwrap();
        let keyring =
            ProductActionDigestKeyringV1::new(key("active-v2", 41), [key("retired-v1", 19)])
                .unwrap();
        let candidates = writer_digest_candidates_v1(
            &keyring,
            input(
                &tenant_id,
                &installation_id,
                &principal_id,
                &session_id,
                &idempotency_key,
                &human_message,
            ),
        );
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].key_id(), "active-v2");
        assert_eq!(candidates[1].key_id(), "retired-v1");
        assert_ne!(candidates[0], candidates[1]);
        assert_eq!(
            format!("{:?}", candidates[0]),
            "WriterDigestCandidateV1(<redacted>)"
        );
        let changed_message = AuthoringHumanMessageV1::parse("Create another room").unwrap();
        let changed = writer_digest_candidates_v1(
            &keyring,
            input(
                &tenant_id,
                &installation_id,
                &principal_id,
                &session_id,
                &idempotency_key,
                &changed_message,
            ),
        );
        assert_eq!(candidates[0].request_digest(), changed[0].request_digest());
        assert_ne!(
            candidates[0].semantic_digest(),
            changed[0].semantic_digest()
        );
        assert_eq!(
            candidates[0].key_fingerprint(),
            changed[0].key_fingerprint()
        );
    }

    #[test]
    fn safe_projection_digest_is_domain_bound_and_deterministic() {
        let first = safe_projection_digest_v1(br#"{"state":"discussion"}"#);
        let second = safe_projection_digest_v1(br#"{"state":"discussion"}"#);
        let changed = safe_projection_digest_v1(br#"{"state":"needs_input"}"#);
        assert_eq!(first, second);
        assert_ne!(first, changed);
        assert_eq!(first.len(), 64);
    }
}
