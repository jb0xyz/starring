use std::fmt::{Debug, Formatter};

use authoring_promotion::{AuthoringSessionId, SessionGeneration};
use design_harness::{
    verify_preview_ruleset_v1, DraftSummary, IntentRecipeReceiptV2, LlmCompletionProvenanceV1,
    PreviewReadyArtifactV1,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const SAFE_PROJECTION_SCHEMA_VERSION: u16 = 1;
const MAX_SAFE_PROJECTION_BYTES: usize = 256 * 1024;
const MAX_ASSISTANT_MESSAGE_BYTES: usize = 4_000;
const MAX_ASSISTANT_MESSAGE_SCALARS: usize = 1_000;
const MAX_CAPABILITIES: usize = 32;
const MAX_CAPABILITY_BYTES: usize = 256;
const MAX_CAPABILITY_SCALARS: usize = 128;
const MAX_DRAFT_ITEMS: usize = 1_024;
const MAX_UNRESOLVED_REFERENCES: usize = 1_024;
const MAX_REFERENCE_BYTES: usize = 256;
const MAX_MODEL_COMPLETIONS: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafeAuthoringTurnStateV1 {
    NeedsInput,
    Discussion,
    CapabilityGap,
    Unsupported,
    Rejected,
    PreviewReady,
}

#[derive(Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SafeAuthoringPreviewV1 {
    revision: u64,
    draft: DraftSummary,
    ruleset: Value,
    receipt: IntentRecipeReceiptV2,
}

impl SafeAuthoringPreviewV1 {
    pub(crate) fn from_artifact(artifact: &PreviewReadyArtifactV1) -> Self {
        Self {
            revision: artifact.preview().revision,
            draft: artifact.preview().draft.clone(),
            ruleset: artifact.preview().ruleset.clone(),
            receipt: artifact.receipt().clone(),
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn draft(&self) -> &DraftSummary {
        &self.draft
    }

    pub fn ruleset(&self) -> &Value {
        &self.ruleset
    }

    pub fn receipt(&self) -> &IntentRecipeReceiptV2 {
        &self.receipt
    }

    fn matches_artifact(&self, artifact: &PreviewReadyArtifactV1) -> bool {
        self.revision == artifact.preview().revision
            && self.draft == artifact.preview().draft
            && self.receipt == *artifact.receipt()
            && serde_json::to_value(artifact.ruleset()).is_ok_and(|ruleset| ruleset == self.ruleset)
    }
}

impl Debug for SafeAuthoringPreviewV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SafeAuthoringPreviewV1(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SafeAuthoringTurnProjectionV1 {
    schema_version: u16,
    state: SafeAuthoringTurnStateV1,
    assistant_message: String,
    capabilities: Vec<String>,
    draft: DraftSummary,
    preview: Option<SafeAuthoringPreviewV1>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    model_completions: Vec<LlmCompletionProvenanceV1>,
}

impl SafeAuthoringTurnProjectionV1 {
    pub(crate) fn from_turn(
        state: SafeAuthoringTurnStateV1,
        assistant_message: String,
        capabilities: Vec<String>,
        draft: DraftSummary,
        preview: Option<SafeAuthoringPreviewV1>,
        model_completions: Vec<LlmCompletionProvenanceV1>,
    ) -> Result<Self, SafeAuthoringProjectionError> {
        let projection = Self {
            schema_version: SAFE_PROJECTION_SCHEMA_VERSION,
            state,
            assistant_message,
            capabilities,
            draft,
            preview,
            model_completions,
        };
        projection.validate()?;
        Ok(projection)
    }

    pub fn state(&self) -> SafeAuthoringTurnStateV1 {
        self.state
    }

    pub fn assistant_message(&self) -> &str {
        &self.assistant_message
    }

    pub fn capabilities(&self) -> &[String] {
        &self.capabilities
    }

    pub fn draft(&self) -> &DraftSummary {
        &self.draft
    }

    pub fn preview(&self) -> Option<&SafeAuthoringPreviewV1> {
        self.preview.as_ref()
    }

    pub fn model_completions(&self) -> &[LlmCompletionProvenanceV1] {
        &self.model_completions
    }

    pub fn to_canonical_json(&self) -> Result<Vec<u8>, SafeAuthoringProjectionError> {
        self.validate_fields()?;
        let bytes = serde_json::to_vec(self)
            .map_err(|_| SafeAuthoringProjectionError::SerializationFailed)?;
        if bytes.len() > MAX_SAFE_PROJECTION_BYTES {
            return Err(SafeAuthoringProjectionError::TooLarge);
        }
        Ok(bytes)
    }

    pub fn from_canonical_json(bytes: &[u8]) -> Result<Self, SafeAuthoringProjectionError> {
        if bytes.len() > MAX_SAFE_PROJECTION_BYTES {
            return Err(SafeAuthoringProjectionError::TooLarge);
        }
        let wire = serde_json::from_slice::<SafeAuthoringProjectionWireV1>(bytes)
            .map_err(|_| SafeAuthoringProjectionError::Malformed)?;
        let projection = Self {
            schema_version: wire.schema_version,
            state: wire.state,
            assistant_message: wire.assistant_message,
            capabilities: wire.capabilities,
            draft: wire.draft,
            preview: wire.preview.map(SafeAuthoringPreviewV1::from),
            model_completions: wire.model_completions,
        };
        projection.validate()?;
        if projection.to_canonical_json()?.as_slice() != bytes {
            return Err(SafeAuthoringProjectionError::NonCanonical);
        }
        Ok(projection)
    }

    pub fn validate(&self) -> Result<(), SafeAuthoringProjectionError> {
        self.validate_fields()?;
        if serde_json::to_vec(self)
            .map_err(|_| SafeAuthoringProjectionError::SerializationFailed)?
            .len()
            > MAX_SAFE_PROJECTION_BYTES
        {
            return Err(SafeAuthoringProjectionError::TooLarge);
        }
        Ok(())
    }

    pub fn validate_preview_integrity(&self) -> Result<(), SafeAuthoringProjectionError> {
        self.validate()?;
        if let Some(preview) = &self.preview {
            verify_preview_ruleset_v1(&preview.ruleset, &preview.receipt.candidate_ruleset_hash)
                .map_err(|_| SafeAuthoringProjectionError::InvalidPreview)?;
        }
        Ok(())
    }

    pub(crate) fn validate_for_storage(
        &self,
        preview_ready_artifact: Option<&PreviewReadyArtifactV1>,
    ) -> Result<(), SafeAuthoringProjectionError> {
        if matches!(
            self.state,
            SafeAuthoringTurnStateV1::Unsupported | SafeAuthoringTurnStateV1::Rejected
        ) {
            return Err(SafeAuthoringProjectionError::NonDurableState);
        }
        self.validate_artifact_binding(preview_ready_artifact)
    }

    pub(crate) fn validate_artifact_binding(
        &self,
        preview_ready_artifact: Option<&PreviewReadyArtifactV1>,
    ) -> Result<(), SafeAuthoringProjectionError> {
        self.validate()?;
        match (self.state, preview_ready_artifact) {
            (SafeAuthoringTurnStateV1::PreviewReady, Some(artifact))
                if self
                    .preview
                    .as_ref()
                    .is_some_and(|preview| preview.matches_artifact(artifact)) =>
            {
                Ok(())
            }
            (SafeAuthoringTurnStateV1::PreviewReady, _) | (_, Some(_)) => {
                Err(SafeAuthoringProjectionError::PreviewArtifactMismatch)
            }
            (_, None) => Ok(()),
        }
    }

    fn validate_fields(&self) -> Result<(), SafeAuthoringProjectionError> {
        if self.schema_version != SAFE_PROJECTION_SCHEMA_VERSION {
            return Err(SafeAuthoringProjectionError::InvalidSchemaVersion);
        }
        validate_text(
            &self.assistant_message,
            MAX_ASSISTANT_MESSAGE_BYTES,
            MAX_ASSISTANT_MESSAGE_SCALARS,
        )?;
        validate_draft(&self.draft)?;
        if self.capabilities.len() > MAX_CAPABILITIES {
            return Err(SafeAuthoringProjectionError::InvalidCapabilities);
        }
        if self.model_completions.len() > MAX_MODEL_COMPLETIONS {
            return Err(SafeAuthoringProjectionError::InvalidModelCompletions);
        }
        let mut request_ids = std::collections::BTreeSet::new();
        let mut completion_digests = std::collections::BTreeSet::new();
        if self.model_completions.iter().any(|provenance| {
            !request_ids.insert(provenance.request_id())
                || !completion_digests.insert(provenance.completion_sha256())
        }) {
            return Err(SafeAuthoringProjectionError::InvalidModelCompletions);
        }
        for capability in &self.capabilities {
            validate_text(capability, MAX_CAPABILITY_BYTES, MAX_CAPABILITY_SCALARS)
                .map_err(|_| SafeAuthoringProjectionError::InvalidCapabilities)?;
        }
        match self.state {
            SafeAuthoringTurnStateV1::PreviewReady => {
                if self.preview.is_none() || !self.capabilities.is_empty() {
                    return Err(SafeAuthoringProjectionError::InvalidStateShape);
                }
            }
            SafeAuthoringTurnStateV1::CapabilityGap => {
                if self.preview.is_some() || self.capabilities.is_empty() {
                    return Err(SafeAuthoringProjectionError::InvalidStateShape);
                }
            }
            SafeAuthoringTurnStateV1::NeedsInput
            | SafeAuthoringTurnStateV1::Discussion
            | SafeAuthoringTurnStateV1::Unsupported
            | SafeAuthoringTurnStateV1::Rejected => {
                if self.preview.is_some() || !self.capabilities.is_empty() {
                    return Err(SafeAuthoringProjectionError::InvalidStateShape);
                }
            }
        }
        if let Some(preview) = &self.preview {
            if preview.revision == 0
                || preview.revision != preview.receipt.candidate_revision
                || preview.draft != self.draft
                || !preview.ruleset.is_object()
                || !valid_receipt(&preview.receipt)
            {
                return Err(SafeAuthoringProjectionError::InvalidPreview);
            }
        }
        Ok(())
    }
}

impl Debug for SafeAuthoringTurnProjectionV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SafeAuthoringTurnProjectionV1(<redacted>)")
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SafeAuthoringProjectionWireV1 {
    schema_version: u16,
    state: SafeAuthoringTurnStateV1,
    assistant_message: String,
    capabilities: Vec<String>,
    draft: DraftSummary,
    preview: Option<SafeAuthoringPreviewWireV1>,
    #[serde(default)]
    model_completions: Vec<LlmCompletionProvenanceV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SafeAuthoringPreviewWireV1 {
    revision: u64,
    draft: DraftSummary,
    ruleset: Value,
    receipt: IntentRecipeReceiptV2,
}

impl From<SafeAuthoringPreviewWireV1> for SafeAuthoringPreviewV1 {
    fn from(wire: SafeAuthoringPreviewWireV1) -> Self {
        Self {
            revision: wire.revision,
            draft: wire.draft,
            ruleset: wire.ruleset,
            receipt: wire.receipt,
        }
    }
}

fn validate_text(
    value: &str,
    max_bytes: usize,
    max_scalars: usize,
) -> Result<(), SafeAuthoringProjectionError> {
    if value.trim().is_empty()
        || value.len() > max_bytes
        || value.chars().count() > max_scalars
        || value.chars().any(is_forbidden_projection_control)
    {
        return Err(SafeAuthoringProjectionError::InvalidText);
    }
    Ok(())
}

fn is_forbidden_projection_control(character: char) -> bool {
    (character.is_control() && character != '\n')
        || matches!(
            character,
            '\u{061c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
        )
}

fn validate_draft(draft: &DraftSummary) -> Result<(), SafeAuthoringProjectionError> {
    if draft.panels > MAX_DRAFT_ITEMS
        || draft.modals > MAX_DRAFT_ITEMS
        || draft.rules > MAX_DRAFT_ITEMS
        || draft.actions > MAX_DRAFT_ITEMS
        || draft.unresolved_references.len() > MAX_UNRESOLVED_REFERENCES
        || draft.unresolved_references.iter().any(|reference| {
            reference.trim().is_empty()
                || reference.len() > MAX_REFERENCE_BYTES
                || reference.chars().any(is_forbidden_reference_character)
        })
    {
        return Err(SafeAuthoringProjectionError::InvalidDraft);
    }
    Ok(())
}

fn is_forbidden_reference_character(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{061c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
        )
}

fn valid_receipt(receipt: &IntentRecipeReceiptV2) -> bool {
    receipt.identity_revision > 0
        && receipt.intent_revision > 0
        && receipt.candidate_revision > 0
        && receipt.request_evidence_entries > 0
        && receipt.compiled_operations > 0
        && [
            receipt.request_evidence_hash.as_str(),
            receipt.compiler_input_hash.as_str(),
            receipt.semantic_intent_hash.as_str(),
            receipt.compiled_plan_hash.as_str(),
            receipt.candidate_ruleset_hash.as_str(),
            receipt.candidate_draft_hash.as_str(),
        ]
        .into_iter()
        .all(is_lowercase_sha256)
}

fn is_lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SafeAuthoringProjectionError {
    #[error("safe authoring projection schema version is invalid")]
    InvalidSchemaVersion,
    #[error("safe authoring projection text is invalid")]
    InvalidText,
    #[error("safe authoring projection capabilities are invalid")]
    InvalidCapabilities,
    #[error("safe authoring projection model completions are invalid")]
    InvalidModelCompletions,
    #[error("safe authoring projection Draft summary is invalid")]
    InvalidDraft,
    #[error("safe authoring projection state shape is invalid")]
    InvalidStateShape,
    #[error("safe authoring projection state is not durable")]
    NonDurableState,
    #[error("safe authoring preview is invalid")]
    InvalidPreview,
    #[error("safe authoring preview does not match its typed artifact")]
    PreviewArtifactMismatch,
    #[error("safe authoring projection is too large")]
    TooLarge,
    #[error("safe authoring projection could not be serialized")]
    SerializationFailed,
    #[error("safe authoring projection is malformed")]
    Malformed,
    #[error("safe authoring projection is not canonical")]
    NonCanonical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthoringMutationDispositionV1 {
    Created,
    ExactReplay,
}

#[derive(Clone, PartialEq)]
pub enum AuthoringTurnOutcomeV1 {
    Committed(AuthoringTurnReceiptV1),
    NotCommitted(SafeAuthoringTurnProjectionV1),
}

impl AuthoringTurnOutcomeV1 {
    pub fn generation(&self) -> Option<SessionGeneration> {
        match self {
            Self::Committed(receipt) => Some(receipt.generation()),
            Self::NotCommitted(_) => None,
        }
    }

    pub fn disposition(&self) -> Option<AuthoringMutationDispositionV1> {
        match self {
            Self::Committed(receipt) => Some(receipt.disposition()),
            Self::NotCommitted(_) => None,
        }
    }

    pub fn projection(&self) -> &SafeAuthoringTurnProjectionV1 {
        match self {
            Self::Committed(receipt) => receipt.projection(),
            Self::NotCommitted(projection) => projection,
        }
    }

    pub fn into_committed(
        self,
    ) -> Result<AuthoringTurnReceiptV1, Box<SafeAuthoringTurnProjectionV1>> {
        match self {
            Self::Committed(receipt) => Ok(receipt),
            Self::NotCommitted(projection) => Err(Box::new(projection)),
        }
    }
}

impl Debug for AuthoringTurnOutcomeV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AuthoringTurnOutcomeV1(<redacted>)")
    }
}

#[derive(Clone, PartialEq)]
pub struct AuthoringTurnReceiptV1 {
    session_id: AuthoringSessionId,
    generation: SessionGeneration,
    disposition: AuthoringMutationDispositionV1,
    projection: SafeAuthoringTurnProjectionV1,
}

impl Debug for AuthoringTurnReceiptV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AuthoringTurnReceiptV1(<redacted>)")
    }
}

impl AuthoringTurnReceiptV1 {
    pub(crate) fn from_result(
        session_id: AuthoringSessionId,
        generation: SessionGeneration,
        disposition: AuthoringMutationDispositionV1,
        projection: SafeAuthoringTurnProjectionV1,
    ) -> Result<Self, SafeAuthoringProjectionError> {
        projection.validate()?;
        Ok(Self {
            session_id,
            generation,
            disposition,
            projection,
        })
    }

    pub fn session_id(&self) -> &AuthoringSessionId {
        &self.session_id
    }

    pub fn generation(&self) -> SessionGeneration {
        self.generation
    }

    pub fn disposition(&self) -> AuthoringMutationDispositionV1 {
        self.disposition
    }

    pub fn projection(&self) -> &SafeAuthoringTurnProjectionV1 {
        &self.projection
    }
}
