use std::fmt::{Debug, Formatter};
use std::sync::Arc;

use automation_runtime_interaction::{
    InteractionEffectCorrelationClassV1, InteractionEffectExpectedPostimageDigestV1,
    InteractionEffectIndeterminateClassV1, InteractionEffectKnownFailureClassV1,
    InteractionEffectKnownFailureV1, InteractionEffectObservedOutputV1,
    InteractionEffectPayloadDigestV1, InteractionReceiptIdentityV1, InteractionTokenV1,
};
use twilight_http::{error::ErrorType, Client};
use twilight_model::channel::Message;
use twilight_model::id::{marker::ApplicationMarker, Id};

use crate::discord_effect_postimage::response_postimage_digest_v1;
use crate::discord_effects::{DiscordEffectObservationEvidenceV1, DiscordEffectReadFailureV1};

pub struct DiscordOriginalResponseObservationRequestV1 {
    receipt_identity: InteractionReceiptIdentityV1,
    interaction_token: InteractionTokenV1,
    expected_postimage: InteractionEffectExpectedPostimageDigestV1,
    persisted_payload_digest: InteractionEffectPayloadDigestV1,
}

impl DiscordOriginalResponseObservationRequestV1 {
    pub fn new(
        receipt_identity: InteractionReceiptIdentityV1,
        interaction_token: InteractionTokenV1,
        expected_postimage: InteractionEffectExpectedPostimageDigestV1,
        persisted_payload_digest: InteractionEffectPayloadDigestV1,
    ) -> Self {
        Self {
            receipt_identity,
            interaction_token,
            expected_postimage,
            persisted_payload_digest,
        }
    }

    fn into_parts(
        self,
    ) -> (
        InteractionReceiptIdentityV1,
        InteractionTokenV1,
        InteractionEffectExpectedPostimageDigestV1,
        InteractionEffectPayloadDigestV1,
    ) {
        (
            self.receipt_identity,
            self.interaction_token,
            self.expected_postimage,
            self.persisted_payload_digest,
        )
    }
}

impl Debug for DiscordOriginalResponseObservationRequestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DiscordOriginalResponseObservationRequestV1(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiscordOriginalResponseObservationOutcomeV1 {
    ExactMatch {
        output: InteractionEffectObservedOutputV1,
        evidence: DiscordEffectObservationEvidenceV1,
    },
    ExactAbsence {
        evidence: DiscordEffectObservationEvidenceV1,
    },
    Conflict {
        evidence: DiscordEffectObservationEvidenceV1,
    },
    Unavailable(DiscordEffectReadFailureV1),
}

impl DiscordOriginalResponseObservationOutcomeV1 {
    pub fn evidence(&self) -> Option<DiscordEffectObservationEvidenceV1> {
        match self {
            Self::ExactMatch { evidence, .. }
            | Self::ExactAbsence { evidence }
            | Self::Conflict { evidence } => Some(*evidence),
            Self::Unavailable(_) => None,
        }
    }
}

#[allow(async_fn_in_trait)]
pub trait DiscordOriginalResponseObserverV1 {
    async fn observe_original_response_v1(
        &self,
        request: DiscordOriginalResponseObservationRequestV1,
    ) -> DiscordOriginalResponseObservationOutcomeV1;
}

#[derive(Clone)]
pub struct OwnedTwilightOriginalResponseObserverV1 {
    http: Arc<Client>,
}

impl OwnedTwilightOriginalResponseObserverV1 {
    pub(crate) fn new(http: Arc<Client>) -> Self {
        Self { http }
    }
}

impl Debug for OwnedTwilightOriginalResponseObserverV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("OwnedTwilightOriginalResponseObserverV1(<redacted>)")
    }
}

impl DiscordOriginalResponseObserverV1 for OwnedTwilightOriginalResponseObserverV1 {
    async fn observe_original_response_v1(
        &self,
        request: DiscordOriginalResponseObservationRequestV1,
    ) -> DiscordOriginalResponseObservationOutcomeV1 {
        let (receipt, token, expected_postimage, persisted_payload_digest) = request.into_parts();
        let application_id = Id::<ApplicationMarker>::new(receipt.application_id().get());
        let response = self
            .http
            .interaction(application_id)
            .response(token.expose_secret())
            .await;
        drop(token);
        let response = match response {
            Ok(response) => response,
            Err(error) => return classify_request_error_v1(&error),
        };
        let message = match response.model().await {
            Ok(message) => message,
            Err(_) => {
                return DiscordOriginalResponseObservationOutcomeV1::Unavailable(
                    malformed_body_failure_v1(),
                )
            }
        };
        classify_message_v1(
            receipt,
            &expected_postimage,
            persisted_payload_digest,
            &message,
        )
    }
}

fn classify_request_error_v1(
    error: &twilight_http::Error,
) -> DiscordOriginalResponseObservationOutcomeV1 {
    match error.kind() {
        ErrorType::Response { status, .. } => classify_http_status_v1(status.get()),
        kind => DiscordOriginalResponseObservationOutcomeV1::Unavailable(
            classify_transport_failure_v1(kind),
        ),
    }
}

fn classify_http_status_v1(status: u16) -> DiscordOriginalResponseObservationOutcomeV1 {
    if status == 404 {
        return DiscordOriginalResponseObservationOutcomeV1::ExactAbsence {
            evidence: absence_evidence_v1(),
        };
    }
    let failure = match status {
        400 => known_failure_v1(
            InteractionEffectKnownFailureClassV1::InvalidRequest,
            Some(status),
        ),
        401 | 403 => known_failure_v1(
            InteractionEffectKnownFailureClassV1::Forbidden,
            Some(status),
        ),
        409 => known_failure_v1(InteractionEffectKnownFailureClassV1::Conflict, Some(status)),
        429 => known_failure_v1(
            InteractionEffectKnownFailureClassV1::RateLimitedBeforeDispatch,
            Some(status),
        ),
        500..=599 => DiscordEffectReadFailureV1::Indeterminate(
            InteractionEffectIndeterminateClassV1::ProviderUnavailable,
        ),
        client_error if (400..=499).contains(&client_error) => {
            known_failure_v1(InteractionEffectKnownFailureClassV1::Rejected, Some(status))
        }
        _ => DiscordEffectReadFailureV1::Indeterminate(
            InteractionEffectIndeterminateClassV1::Unknown,
        ),
    };
    DiscordOriginalResponseObservationOutcomeV1::Unavailable(failure)
}

fn classify_transport_failure_v1(kind: &ErrorType) -> DiscordEffectReadFailureV1 {
    match kind {
        ErrorType::BuildingRequest
        | ErrorType::CreatingHeader { .. }
        | ErrorType::Json
        | ErrorType::Validation => known_failure_v1(
            InteractionEffectKnownFailureClassV1::InvalidRequest,
            Some(400),
        ),
        ErrorType::Unauthorized => {
            known_failure_v1(InteractionEffectKnownFailureClassV1::Forbidden, Some(401))
        }
        ErrorType::RequestTimedOut => DiscordEffectReadFailureV1::Indeterminate(
            InteractionEffectIndeterminateClassV1::DeadlineElapsed,
        ),
        ErrorType::RequestError => DiscordEffectReadFailureV1::Indeterminate(
            InteractionEffectIndeterminateClassV1::ConnectionLost,
        ),
        ErrorType::RequestCanceled => DiscordEffectReadFailureV1::Indeterminate(
            InteractionEffectIndeterminateClassV1::Cancelled,
        ),
        ErrorType::Parsing { .. } => malformed_body_failure_v1(),
        ErrorType::Response { status, .. } => match classify_http_status_v1(status.get()) {
            DiscordOriginalResponseObservationOutcomeV1::Unavailable(failure) => failure,
            DiscordOriginalResponseObservationOutcomeV1::ExactAbsence { .. } => {
                known_failure_v1(InteractionEffectKnownFailureClassV1::NotFound, Some(404))
            }
            _ => DiscordEffectReadFailureV1::Indeterminate(
                InteractionEffectIndeterminateClassV1::Unknown,
            ),
        },
        _ => DiscordEffectReadFailureV1::Indeterminate(
            InteractionEffectIndeterminateClassV1::Unknown,
        ),
    }
}

fn malformed_body_failure_v1() -> DiscordEffectReadFailureV1 {
    DiscordEffectReadFailureV1::Indeterminate(
        InteractionEffectIndeterminateClassV1::MalformedResponse,
    )
}

fn known_failure_v1(
    class: InteractionEffectKnownFailureClassV1,
    status: Option<u16>,
) -> DiscordEffectReadFailureV1 {
    match InteractionEffectKnownFailureV1::new(class, status) {
        Ok(failure) => DiscordEffectReadFailureV1::KnownFailed(failure),
        Err(_) => DiscordEffectReadFailureV1::Indeterminate(
            InteractionEffectIndeterminateClassV1::Unknown,
        ),
    }
}

#[allow(deprecated)]
fn classify_message_v1(
    receipt: InteractionReceiptIdentityV1,
    expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
    persisted_payload_digest: InteractionEffectPayloadDigestV1,
    message: &Message,
) -> DiscordOriginalResponseObservationOutcomeV1 {
    let metadata_interaction = message
        .interaction_metadata
        .as_ref()
        .map(|metadata| metadata.id.get());
    let legacy_interaction = message
        .interaction
        .as_ref()
        .map(|interaction| interaction.id.get());
    classify_observed_response_v1(
        receipt,
        expected_postimage,
        persisted_payload_digest,
        message.application_id.map(Id::get),
        metadata_interaction,
        legacy_interaction,
        &message.content,
    )
}

#[allow(clippy::too_many_arguments)]
fn classify_observed_response_v1(
    receipt: InteractionReceiptIdentityV1,
    expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
    persisted_payload_digest: InteractionEffectPayloadDigestV1,
    observed_application_id: Option<u64>,
    metadata_interaction: Option<u64>,
    legacy_interaction: Option<u64>,
    content: &str,
) -> DiscordOriginalResponseObservationOutcomeV1 {
    let actor_identity_matches = observed_application_id == Some(receipt.application_id().get());
    let target_identity_matches = match (metadata_interaction, legacy_interaction) {
        (Some(metadata), Some(legacy)) => {
            metadata == receipt.interaction_id().get() && legacy == receipt.interaction_id().get()
        }
        (Some(interaction), None) | (None, Some(interaction)) => {
            interaction == receipt.interaction_id().get()
        }
        (None, None) => false,
    };
    let postimage_matches = response_postimage_digest_v1(content) == *expected_postimage;
    classify_snapshot_v1(
        receipt,
        persisted_payload_digest,
        target_identity_matches,
        actor_identity_matches,
        postimage_matches,
    )
}

fn classify_snapshot_v1(
    receipt: InteractionReceiptIdentityV1,
    persisted_payload_digest: InteractionEffectPayloadDigestV1,
    target_identity_matches: bool,
    actor_identity_matches: bool,
    postimage_matches: bool,
) -> DiscordOriginalResponseObservationOutcomeV1 {
    if target_identity_matches && actor_identity_matches && postimage_matches {
        return DiscordOriginalResponseObservationOutcomeV1::ExactMatch {
            output: InteractionEffectObservedOutputV1::OriginalResponse {
                receipt_identity: receipt,
                payload_digest: persisted_payload_digest,
            },
            evidence: exact_evidence_v1(),
        };
    }
    DiscordOriginalResponseObservationOutcomeV1::Conflict {
        evidence: DiscordEffectObservationEvidenceV1::new(
            InteractionEffectCorrelationClassV1::InteractionReceipt,
            u16::from(target_identity_matches && actor_identity_matches),
            1,
            target_identity_matches,
            actor_identity_matches,
            postimage_matches,
        ),
    }
}

fn exact_evidence_v1() -> DiscordEffectObservationEvidenceV1 {
    DiscordEffectObservationEvidenceV1::new(
        InteractionEffectCorrelationClassV1::InteractionReceipt,
        1,
        0,
        true,
        true,
        true,
    )
}

fn absence_evidence_v1() -> DiscordEffectObservationEvidenceV1 {
    DiscordEffectObservationEvidenceV1::new(
        InteractionEffectCorrelationClassV1::InteractionReceipt,
        0,
        0,
        false,
        false,
        false,
    )
}

#[cfg(test)]
mod tests {
    use automation_runtime_interaction::{
        DiscordApplicationIdV1, DiscordInteractionIdV1, InteractionEffectKnownFailureClassV1,
    };
    use static_assertions::assert_not_impl_any;

    use super::*;

    fn digest(value: char) -> String {
        value.to_string().repeat(64)
    }

    fn receipt() -> InteractionReceiptIdentityV1 {
        InteractionReceiptIdentityV1::new(
            DiscordApplicationIdV1::new(10).unwrap(),
            DiscordInteractionIdV1::new(20).unwrap(),
        )
    }

    fn payload() -> InteractionEffectPayloadDigestV1 {
        InteractionEffectPayloadDigestV1::parse(digest('a')).unwrap()
    }

    #[test]
    fn exact_snapshot_binds_persisted_payload_only_after_identity_and_postimage_match() {
        let expected = response_postimage_digest_v1("completed");
        let exact = classify_observed_response_v1(
            receipt(),
            &expected,
            payload(),
            Some(10),
            Some(20),
            None,
            "completed",
        );
        let DiscordOriginalResponseObservationOutcomeV1::ExactMatch { output, evidence } = exact
        else {
            panic!("expected exact response")
        };
        assert_eq!(
            output,
            InteractionEffectObservedOutputV1::OriginalResponse {
                receipt_identity: receipt(),
                payload_digest: payload(),
            }
        );
        assert_eq!(evidence.exact_correlation_matches(), 1);
        assert_eq!(evidence.conflicting_matches(), 0);
        assert!(evidence.target_identity_matches());
        assert!(evidence.actor_identity_matches());
        assert!(evidence.postimage_matches());

        for (application_id, interaction_id, content) in [
            (Some(11), Some(20), "completed"),
            (Some(10), Some(21), "completed"),
            (Some(10), Some(20), "different"),
        ] {
            let conflict = classify_observed_response_v1(
                receipt(),
                &expected,
                payload(),
                application_id,
                interaction_id,
                None,
                content,
            );
            assert!(matches!(
                conflict,
                DiscordOriginalResponseObservationOutcomeV1::Conflict { .. }
            ));
        }
    }

    #[test]
    fn read_statuses_are_fail_closed_and_only_not_found_proves_absence() {
        let absence = classify_http_status_v1(404);
        let DiscordOriginalResponseObservationOutcomeV1::ExactAbsence { evidence } = absence else {
            panic!("expected exact absence")
        };
        assert_eq!(evidence.exact_correlation_matches(), 0);
        assert_eq!(evidence.conflicting_matches(), 0);

        for (status, class) in [
            (403, InteractionEffectKnownFailureClassV1::Forbidden),
            (
                429,
                InteractionEffectKnownFailureClassV1::RateLimitedBeforeDispatch,
            ),
        ] {
            let DiscordOriginalResponseObservationOutcomeV1::Unavailable(
                DiscordEffectReadFailureV1::KnownFailed(failure),
            ) = classify_http_status_v1(status)
            else {
                panic!("expected typed read failure")
            };
            assert_eq!(failure.class(), class);
            assert_eq!(failure.http_status(), Some(status));
        }

        assert_eq!(
            classify_http_status_v1(500),
            DiscordOriginalResponseObservationOutcomeV1::Unavailable(
                DiscordEffectReadFailureV1::Indeterminate(
                    InteractionEffectIndeterminateClassV1::ProviderUnavailable,
                ),
            )
        );
    }

    #[test]
    fn network_and_malformed_reads_remain_indeterminate() {
        assert_eq!(
            classify_transport_failure_v1(&ErrorType::RequestError),
            DiscordEffectReadFailureV1::Indeterminate(
                InteractionEffectIndeterminateClassV1::ConnectionLost,
            )
        );
        assert_eq!(
            classify_transport_failure_v1(&ErrorType::Parsing { body: Vec::new() }),
            DiscordEffectReadFailureV1::Indeterminate(
                InteractionEffectIndeterminateClassV1::MalformedResponse,
            )
        );
        assert_eq!(
            malformed_body_failure_v1(),
            DiscordEffectReadFailureV1::Indeterminate(
                InteractionEffectIndeterminateClassV1::MalformedResponse,
            )
        );
    }

    #[test]
    fn observation_request_owns_and_redacts_the_decrypted_token() {
        let request = DiscordOriginalResponseObservationRequestV1::new(
            receipt(),
            InteractionTokenV1::new("short-lived-secret").unwrap(),
            response_postimage_digest_v1("done"),
            payload(),
        );
        let debug = format!("{request:?}");
        assert_eq!(
            debug,
            "DiscordOriginalResponseObservationRequestV1(<redacted>)"
        );
        assert!(!debug.contains("short-lived-secret"));
    }

    assert_not_impl_any!(DiscordOriginalResponseObservationRequestV1: Clone, serde::Serialize);
}
