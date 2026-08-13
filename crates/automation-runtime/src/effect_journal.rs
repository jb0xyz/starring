pub use automation_runtime_effect_contract::{
    InteractionEffectJournalPlanBindErrorV1, InteractionEffectJournalPlanEntryV1,
    InteractionEffectJournalPlanV1,
};
use automation_runtime_interaction::{
    InteractionEffectAttemptOutcomeV1, InteractionEffectMaterializedPlanV1,
    InteractionInstanceManifestDigestV1,
};

pub struct InteractionEffectJournalIntendV1<'a> {
    materialized: &'a InteractionEffectMaterializedPlanV1,
    resolved_instance_manifest_digest: Option<&'a InteractionInstanceManifestDigestV1>,
}

impl<'a> InteractionEffectJournalIntendV1<'a> {
    pub fn new(
        materialized: &'a InteractionEffectMaterializedPlanV1,
        resolved_instance_manifest_digest: Option<&'a InteractionInstanceManifestDigestV1>,
    ) -> Self {
        Self {
            materialized,
            resolved_instance_manifest_digest,
        }
    }

    pub fn materialized(&self) -> &InteractionEffectMaterializedPlanV1 {
        self.materialized
    }

    pub fn resolved_instance_manifest_digest(
        &self,
    ) -> Option<&InteractionInstanceManifestDigestV1> {
        self.resolved_instance_manifest_digest
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionEffectPlanBindDispositionV1 {
    Fresh,
    ExactReplay,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InteractionEffectIntentDispositionV1<P> {
    ExternalCallAuthorized(P),
    ExactReplay,
}

#[allow(async_fn_in_trait)]
pub trait InteractionEffectJournalPortV1 {
    type Error;
    type IntentPermit;

    async fn bind_effect_plan_v1(
        &self,
        plan: &InteractionEffectJournalPlanV1,
    ) -> Result<InteractionEffectPlanBindDispositionV1, Self::Error>;

    async fn intend_effect_v1(
        &self,
        intent: InteractionEffectJournalIntendV1<'_>,
    ) -> Result<InteractionEffectIntentDispositionV1<Self::IntentPermit>, Self::Error>;

    async fn finish_effect_v1(
        &self,
        permit: &Self::IntentPermit,
        materialized: &InteractionEffectMaterializedPlanV1,
        outcome: &InteractionEffectAttemptOutcomeV1,
    ) -> Result<(), Self::Error>;
}
