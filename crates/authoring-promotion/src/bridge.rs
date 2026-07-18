use std::num::NonZeroU64;

use automation_ruleset::{RuleSetStore, RuleSetStoreError};
use automation_ruleset_activation::{
    ActivationApprovalContextV1, ActivationLinkStateV1, ActivationRequest, ActivationRequestStore,
    ActivationStoreError, ApprovalBindingContextV1, ExpectedActiveBaselineV1, LinkProductError,
};
use desired_state::ResourceKey;
use resource_resolution::{
    approval_binding_fingerprint_v1, resource_binding_fingerprint_v2, ResolvedApprovalBinding,
    ResourceBindingMap,
};

use crate::{
    EnsurePendingActivationV1, LinkPendingActivationV1, PendingActivationDispositionV1,
    PendingActivationPort, PendingActivationPortError, PendingActivationReceiptV1,
    ResolveProductApprovalContextV1, ResolvedProductApprovalContextV1,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProductApprovalEnvironmentV1 {
    pub binding_revision: NonZeroU64,
    pub bindings: ResourceBindingMap,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductApprovalEnvironmentError {
    #[error("product approval environment could not be loaded: {0}")]
    Load(String),
}

#[allow(async_fn_in_trait)]
pub trait ProductApprovalEnvironmentProvider {
    async fn load_fresh(
        &self,
        request: &ResolveProductApprovalContextV1,
    ) -> Result<ProductApprovalEnvironmentV1, ProductApprovalEnvironmentError>;
}

pub struct ProductActivationBridge<'a, R, E, A> {
    rulesets: &'a R,
    environment: &'a E,
    requests: &'a A,
}

impl<'a, R, E, A> ProductActivationBridge<'a, R, E, A> {
    pub fn new(rulesets: &'a R, environment: &'a E, requests: &'a A) -> Self {
        Self {
            rulesets,
            environment,
            requests,
        }
    }
}

impl<R, E, A> PendingActivationPort for ProductActivationBridge<'_, R, E, A>
where
    R: RuleSetStore,
    E: ProductApprovalEnvironmentProvider,
    A: ActivationRequestStore,
{
    async fn resolve_product_approval_context(
        &self,
        request: ResolveProductApprovalContextV1,
    ) -> Result<ResolvedProductApprovalContextV1, PendingActivationPortError> {
        let target = self
            .rulesets
            .get_version(
                request.target.guild_id,
                &request.target.ruleset_key,
                request.target.version,
            )
            .await
            .map_err(ruleset_error)?
            .ok_or_else(|| conflict("target RuleSet version is missing"))?;
        if target.guild_id != request.target.guild_id
            || target.ruleset_key != request.target.ruleset_key
            || target.version != request.target.version
            || target.content_hash != request.target.content_hash
        {
            return Err(conflict("target RuleSet identity does not match"));
        }
        let environment = self
            .environment
            .load_fresh(&request)
            .await
            .map_err(|error| PendingActivationPortError::Backend(error.to_string()))?;
        let expected_revision = NonZeroU64::new(request.binding_revision.get())
            .ok_or_else(|| conflict("authoring binding revision is invalid"))?;
        if environment.binding_revision != expected_revision {
            return Err(conflict("authoring binding revision drifted"));
        }
        if resource_binding_fingerprint_v2(&environment.bindings) != request.context_fingerprint {
            return Err(conflict("authoring resource context drifted"));
        }
        if !request
            .required_channel_bindings
            .windows(2)
            .all(|window| window[0] < window[1])
        {
            return Err(conflict("required channel bindings are not canonical"));
        }
        let required_bindings = request
            .required_channel_bindings
            .iter()
            .map(|key| {
                let resource_key = ResourceKey(key.clone());
                environment
                    .bindings
                    .channel_bindings
                    .get(&resource_key)
                    .copied()
                    .map(|id| ResolvedApprovalBinding::Channel {
                        key: resource_key,
                        id,
                    })
                    .ok_or_else(|| conflict(&format!("required channel binding is missing: {key}")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let fingerprint = approval_binding_fingerprint_v1(
            request.target.guild_id,
            environment.binding_revision,
            &required_bindings,
        )
        .map_err(|error| conflict(&error.to_string()))?;
        let active = self
            .rulesets
            .active(request.target.guild_id, &request.target.ruleset_key)
            .await
            .map_err(ruleset_error)?;
        let baseline = active
            .as_ref()
            .map_or(ExpectedActiveBaselineV1::Absent, |active| {
                ExpectedActiveBaselineV1::Exact {
                    version: active.version,
                    content_hash: active.content_hash,
                }
            });
        Ok(ResolvedProductApprovalContextV1 {
            binding: ApprovalBindingContextV1 {
                revision: environment.binding_revision,
                required_bindings,
                fingerprint,
            },
            baseline,
        })
    }

    async fn ensure_pending_activation(
        &self,
        request: EnsurePendingActivationV1,
    ) -> Result<PendingActivationReceiptV1, PendingActivationPortError> {
        match self.requests.create_product(request.create.clone()).await {
            Ok(created) => Ok(PendingActivationReceiptV1 {
                request: created,
                disposition: PendingActivationDispositionV1::Created,
            }),
            Err(ActivationStoreError::DuplicateRequest) => {
                let existing = self
                    .requests
                    .get(&request.create.id)
                    .await
                    .map_err(activation_error)?
                    .ok_or_else(|| {
                        PendingActivationPortError::Indeterminate(
                            "duplicate product request disappeared".to_string(),
                        )
                    })?;
                if !matches_product_request(&existing, &request.create) {
                    return Err(conflict("product activation identity mismatch"));
                }
                Ok(PendingActivationReceiptV1 {
                    request: existing,
                    disposition: PendingActivationDispositionV1::Reused,
                })
            }
            Err(error) => Err(activation_error(error)),
        }
    }

    async fn link_pending_activation(
        &self,
        request: LinkPendingActivationV1,
    ) -> Result<ActivationRequest, PendingActivationPortError> {
        match self
            .requests
            .link_product(&request.request_id, request.link)
            .await
        {
            Ok(linked) => Ok(linked),
            Err(LinkProductError::Expired) => self
                .requests
                .get(&request.request_id)
                .await
                .map_err(activation_error)?
                .ok_or_else(|| {
                    PendingActivationPortError::Indeterminate(
                        "expired product request disappeared".to_string(),
                    )
                }),
            Err(LinkProductError::NotProduct) => {
                Err(conflict("activation request is not product-authored"))
            }
            Err(LinkProductError::Conflict) => {
                Err(conflict("product activation link identity mismatch"))
            }
            Err(LinkProductError::NotPending) => {
                Err(conflict("product activation request is not pending"))
            }
            Err(LinkProductError::Store(error)) => Err(activation_error(error)),
        }
    }
}

fn matches_product_request(
    existing: &ActivationRequest,
    expected: &automation_ruleset_activation::CreateProductActivationRequest,
) -> bool {
    let expected_ttl = i64::try_from(expected.context.policy.ttl_seconds.get())
        .ok()
        .and_then(chrono::Duration::try_seconds);
    existing.id == expected.id
        && existing.target == expected.target
        && existing.requester == expected.requester
        && existing.required_approvals == expected.context.policy.required_approvals.get()
        && existing.observed_active == expected.context.baseline.as_observed()
        && existing.expires_at - existing.created_at == expected_ttl.unwrap_or_default()
        && existing.approval_context
            == (ActivationApprovalContextV1::ProductAuthoring {
                context: Box::new(expected.context.clone()),
            })
        && !matches!(existing.link_state, ActivationLinkStateV1::NotRequired)
}

fn conflict(message: &str) -> PendingActivationPortError {
    PendingActivationPortError::Conflict(message.to_string())
}

fn ruleset_error(error: RuleSetStoreError) -> PendingActivationPortError {
    match error {
        RuleSetStoreError::Backend(message) => PendingActivationPortError::Backend(message),
        error => conflict(&format!("RuleSet authority failed: {error:?}")),
    }
}

fn activation_error(error: ActivationStoreError) -> PendingActivationPortError {
    match error {
        ActivationStoreError::Backend(message) => PendingActivationPortError::Backend(message),
        ActivationStoreError::DuplicateRequest => {
            PendingActivationPortError::Indeterminate("duplicate request race".to_string())
        }
        error => conflict(&error.to_string()),
    }
}
