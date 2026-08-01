use automation_runtime_interaction::{
    InteractionEffectAttemptOutcomeV1, InteractionEffectExpectedPostimageDigestV1,
    InteractionEffectMaterializedPlanV1, InteractionEffectPlanDefinitionV1,
    InteractionInstanceManifestDigestV1, InteractionPreflightCertificateDigestV1,
    InteractionPreflightSnapshotDigestV1,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionEffectJournalPlanEntryV1 {
    definition: InteractionEffectPlanDefinitionV1,
    expected_postimage_digest: InteractionEffectExpectedPostimageDigestV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionEffectJournalPlanV1 {
    preflight_certificate_digest: InteractionPreflightCertificateDigestV1,
    snapshot_digest: InteractionPreflightSnapshotDigestV1,
    entries: Vec<InteractionEffectJournalPlanEntryV1>,
}

impl InteractionEffectJournalPlanV1 {
    pub fn new(
        preflight_certificate_digest: InteractionPreflightCertificateDigestV1,
        snapshot_digest: InteractionPreflightSnapshotDigestV1,
        entries: Vec<InteractionEffectJournalPlanEntryV1>,
    ) -> Self {
        Self {
            preflight_certificate_digest,
            snapshot_digest,
            entries,
        }
    }

    pub fn preflight_certificate_digest(&self) -> &InteractionPreflightCertificateDigestV1 {
        &self.preflight_certificate_digest
    }

    pub fn snapshot_digest(&self) -> &InteractionPreflightSnapshotDigestV1 {
        &self.snapshot_digest
    }

    pub fn entries(&self) -> &[InteractionEffectJournalPlanEntryV1] {
        &self.entries
    }
}

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

impl InteractionEffectJournalPlanEntryV1 {
    pub fn new(
        definition: InteractionEffectPlanDefinitionV1,
        expected_postimage_digest: InteractionEffectExpectedPostimageDigestV1,
    ) -> Self {
        Self {
            definition,
            expected_postimage_digest,
        }
    }

    pub fn definition(&self) -> &InteractionEffectPlanDefinitionV1 {
        &self.definition
    }

    pub fn expected_postimage_digest(&self) -> &InteractionEffectExpectedPostimageDigestV1 {
        &self.expected_postimage_digest
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
