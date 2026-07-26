mod wire;

#[cfg(test)]
mod tests;

use crate::v2_digest::{drain_intent_digest_v2, product_mutation_digest_v2};
use crate::{
    RuntimeCanonicalValueErrorV2, RuntimeDrainIntentDigestV2, RuntimeDrainIntentIdV2,
    RuntimeDrainIntentKeyV2, RuntimeDrainIntentPreimageV2, RuntimeProductMutationDigestV2,
    RuntimeProductMutationPreimageV2,
};

const PRODUCT_MUTATION_MAX_OCTETS: usize = 32_768;
const DRAIN_INTENT_MAX_OCTETS: usize = 65_536;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProductDrainCanonicalRootV2 {
    ProductMutation,
    DrainIntent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProductDrainCanonicalFieldV2 {
    OperationId,
    IntentId,
    ProductMutationDigest,
    TenantId,
    InstallationId,
    DeploymentId,
    ExpectedRevision,
    SlotGuildId,
    SlotRuleSetKey,
    TargetGuildId,
    TargetRuleSetKey,
    TargetVersion,
    TargetContentHash,
    TargetBindingRevision,
    TargetBindingFingerprint,
    MutationKind,
    ProductSemanticRequestDigest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProductDrainCorrelationV2 {
    ProductOperationId,
    ProductMutationDigest,
    Scope,
    ExpectedRevision,
    Slot,
    ExpectedTarget,
    MutationKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeProductDrainCanonicalErrorV2 {
    #[error("runtime {root:?} canonical payload exceeds its size limit")]
    PayloadTooLarge {
        root: RuntimeProductDrainCanonicalRootV2,
    },
    #[error("runtime {root:?} canonical payload encoding failed")]
    Encoding {
        root: RuntimeProductDrainCanonicalRootV2,
    },
    #[error("runtime {root:?} canonical payload decoding failed")]
    Decoding {
        root: RuntimeProductDrainCanonicalRootV2,
    },
    #[error("runtime {root:?} canonical payload format version is unsupported")]
    UnsupportedFormatVersion {
        root: RuntimeProductDrainCanonicalRootV2,
    },
    #[error("runtime {root:?} canonical payload has a noncanonical representation")]
    NonCanonicalEncoding {
        root: RuntimeProductDrainCanonicalRootV2,
    },
    #[error("runtime {root:?} canonical field {field:?} is invalid")]
    InvalidField {
        root: RuntimeProductDrainCanonicalRootV2,
        field: RuntimeProductDrainCanonicalFieldV2,
    },
    #[error("runtime {root:?} canonical field {field:?} is invalid: {reason}")]
    CanonicalValue {
        root: RuntimeProductDrainCanonicalRootV2,
        field: RuntimeProductDrainCanonicalFieldV2,
        reason: RuntimeCanonicalValueErrorV2,
    },
    #[error("runtime {root:?} serving slot does not match its expected target")]
    SlotTargetMismatch {
        root: RuntimeProductDrainCanonicalRootV2,
    },
    #[error("runtime {root:?} persisted digest does not match its canonical payload")]
    PersistedDigestMismatch {
        root: RuntimeProductDrainCanonicalRootV2,
    },
    #[error("runtime Product and drain canonical roots disagree on {field:?}")]
    CorrelationMismatch {
        field: RuntimeProductDrainCorrelationV2,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeCanonicalProductDrainV2 {
    product_preimage: RuntimeProductMutationPreimageV2,
    product_bytes: Box<[u8]>,
    product_digest: RuntimeProductMutationDigestV2,
    drain_preimage: RuntimeDrainIntentPreimageV2,
    drain_bytes: Box<[u8]>,
    drain_digest: RuntimeDrainIntentDigestV2,
}

impl RuntimeCanonicalProductDrainV2 {
    pub fn new(
        product_preimage: RuntimeProductMutationPreimageV2,
        intent_id: RuntimeDrainIntentIdV2,
    ) -> Result<Self, RuntimeProductDrainCanonicalErrorV2> {
        let product_bytes = wire::encode_product_mutation(&product_preimage)?;
        let product_digest = product_mutation_digest_v2(&product_bytes);
        let drain_preimage = RuntimeDrainIntentPreimageV2::from_key(RuntimeDrainIntentKeyV2 {
            intent_id,
            product_operation_id: product_preimage.operation_id.clone(),
            product_mutation_digest: product_digest.clone(),
            scope: product_preimage.scope.clone(),
            expected_revision: product_preimage.expected_revision,
            slot: product_preimage.slot.clone(),
            expected_target: product_preimage.expected_target.clone(),
            mutation_kind: product_preimage.mutation_kind,
        });
        let drain_bytes = wire::encode_drain_intent(&drain_preimage)?;
        let drain_digest = drain_intent_digest_v2(&drain_bytes);
        let canonical = Self {
            product_preimage,
            product_bytes: product_bytes.into_boxed_slice(),
            product_digest,
            drain_preimage,
            drain_bytes: drain_bytes.into_boxed_slice(),
            drain_digest,
        };
        validate_pair(
            &canonical.product_preimage,
            &canonical.product_digest,
            &canonical.drain_preimage,
        )?;
        Ok(canonical)
    }

    pub fn from_persisted(
        product_bytes: &[u8],
        persisted_product_digest: &RuntimeProductMutationDigestV2,
        drain_bytes: &[u8],
        persisted_drain_digest: &RuntimeDrainIntentDigestV2,
    ) -> Result<Self, RuntimeProductDrainCanonicalErrorV2> {
        let product_preimage = wire::decode_product_mutation(product_bytes)?;
        let product_digest = product_mutation_digest_v2(product_bytes);
        if product_digest != *persisted_product_digest {
            return Err(
                RuntimeProductDrainCanonicalErrorV2::PersistedDigestMismatch {
                    root: RuntimeProductDrainCanonicalRootV2::ProductMutation,
                },
            );
        }
        let drain_preimage = wire::decode_drain_intent(drain_bytes)?;
        let drain_digest = drain_intent_digest_v2(drain_bytes);
        if drain_digest != *persisted_drain_digest {
            return Err(
                RuntimeProductDrainCanonicalErrorV2::PersistedDigestMismatch {
                    root: RuntimeProductDrainCanonicalRootV2::DrainIntent,
                },
            );
        }
        validate_pair(&product_preimage, &product_digest, &drain_preimage)?;
        Ok(Self {
            product_preimage,
            product_bytes: product_bytes.to_vec().into_boxed_slice(),
            product_digest,
            drain_preimage,
            drain_bytes: drain_bytes.to_vec().into_boxed_slice(),
            drain_digest,
        })
    }

    pub fn product_preimage(&self) -> &RuntimeProductMutationPreimageV2 {
        &self.product_preimage
    }

    pub fn product_mutation_request_bytes(&self) -> &[u8] {
        &self.product_bytes
    }

    pub fn product_mutation_digest(&self) -> &RuntimeProductMutationDigestV2 {
        &self.product_digest
    }

    pub fn drain_preimage(&self) -> &RuntimeDrainIntentPreimageV2 {
        &self.drain_preimage
    }

    pub fn drain_intent_request_bytes(&self) -> &[u8] {
        &self.drain_bytes
    }

    pub fn drain_intent_digest(&self) -> &RuntimeDrainIntentDigestV2 {
        &self.drain_digest
    }
}

fn validate_product(
    product: &RuntimeProductMutationPreimageV2,
) -> Result<(), RuntimeProductDrainCanonicalErrorV2> {
    if !product.slot.matches_target(&product.expected_target) {
        return Err(RuntimeProductDrainCanonicalErrorV2::SlotTargetMismatch {
            root: RuntimeProductDrainCanonicalRootV2::ProductMutation,
        });
    }
    Ok(())
}

fn validate_drain(
    drain: &RuntimeDrainIntentPreimageV2,
) -> Result<(), RuntimeProductDrainCanonicalErrorV2> {
    if !drain.key.slot.matches_target(&drain.key.expected_target) {
        return Err(RuntimeProductDrainCanonicalErrorV2::SlotTargetMismatch {
            root: RuntimeProductDrainCanonicalRootV2::DrainIntent,
        });
    }
    Ok(())
}

fn validate_pair(
    product: &RuntimeProductMutationPreimageV2,
    product_digest: &RuntimeProductMutationDigestV2,
    drain: &RuntimeDrainIntentPreimageV2,
) -> Result<(), RuntimeProductDrainCanonicalErrorV2> {
    validate_product(product)?;
    validate_drain(drain)?;
    let key = &drain.key;
    if product.operation_id != key.product_operation_id {
        return Err(RuntimeProductDrainCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeProductDrainCorrelationV2::ProductOperationId,
        });
    }
    if *product_digest != key.product_mutation_digest {
        return Err(RuntimeProductDrainCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeProductDrainCorrelationV2::ProductMutationDigest,
        });
    }
    if product.scope != key.scope {
        return Err(RuntimeProductDrainCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeProductDrainCorrelationV2::Scope,
        });
    }
    if product.expected_revision != key.expected_revision {
        return Err(RuntimeProductDrainCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeProductDrainCorrelationV2::ExpectedRevision,
        });
    }
    if product.slot != key.slot {
        return Err(RuntimeProductDrainCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeProductDrainCorrelationV2::Slot,
        });
    }
    if product.expected_target != key.expected_target {
        return Err(RuntimeProductDrainCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeProductDrainCorrelationV2::ExpectedTarget,
        });
    }
    if product.mutation_kind != key.mutation_kind {
        return Err(RuntimeProductDrainCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeProductDrainCorrelationV2::MutationKind,
        });
    }
    Ok(())
}
