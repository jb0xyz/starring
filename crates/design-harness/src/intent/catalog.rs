use std::collections::BTreeSet;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::errors::StructuredError;

use super::model::{PRIVATE_STUDY_ROOM_RECIPE_ID, PRIVATE_STUDY_ROOM_RECIPE_VERSION};

pub(super) const COMPILER_REVISION: u32 = 1;
pub(super) const MAX_COMPILED_REQUIREMENTS: usize = 31;

const PRIVATE_STUDY_ROOM_EXTRACTOR_REVISION: u32 = 3;
const PRIVATE_STUDY_ROOM_NORMALIZER_REVISION: u32 = 1;
const PRIVATE_STUDY_ROOM_SIMULATOR_REVISION: u32 = 1;
const PRIVATE_STUDY_ROOM_MIN_REQUIREMENTS: usize = 22;
const PRIVATE_STUDY_ROOM_MAX_REQUIREMENTS: usize = 26;
const RECIPE_DESCRIPTOR_DIGEST_DOMAIN: &str = "starring.intent.recipe_descriptor.v1";
const RECIPE_REGISTRY_DIGEST_DOMAIN: &str = "starring.intent.recipe_registry.v1";

macro_rules! define_recipe_kinds_v1 {
    ($($kind:ident),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
        #[serde(rename_all = "snake_case")]
        pub enum RecipeKindV1 {
            $($kind),+
        }

        impl RecipeKindV1 {
            const ALL: &'static [Self] = &[$(Self::$kind),+];
        }
    };
}

define_recipe_kinds_v1!(PrivateStudyRoomV1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeDescriptorV1 {
    pub kind: RecipeKindV1,
    pub id: &'static str,
    pub version: u32,
    pub extractor_revision: u32,
    pub normalizer_revision: u32,
    pub compiler_revision: u32,
    pub simulator_revision: u32,
    pub min_requirements: usize,
    pub max_requirements: usize,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct CatalogDigestEnvelope<'a, T: ?Sized> {
    domain: &'static str,
    value: &'a T,
}

pub fn recipe_descriptor_v1(kind: RecipeKindV1) -> RecipeDescriptorV1 {
    match kind {
        RecipeKindV1::PrivateStudyRoomV1 => RecipeDescriptorV1 {
            kind,
            id: PRIVATE_STUDY_ROOM_RECIPE_ID,
            version: PRIVATE_STUDY_ROOM_RECIPE_VERSION,
            extractor_revision: PRIVATE_STUDY_ROOM_EXTRACTOR_REVISION,
            normalizer_revision: PRIVATE_STUDY_ROOM_NORMALIZER_REVISION,
            compiler_revision: COMPILER_REVISION,
            simulator_revision: PRIVATE_STUDY_ROOM_SIMULATOR_REVISION,
            min_requirements: PRIVATE_STUDY_ROOM_MIN_REQUIREMENTS,
            max_requirements: PRIVATE_STUDY_ROOM_MAX_REQUIREMENTS,
        },
    }
}

pub fn recipe_registry_v1() -> Result<Vec<RecipeDescriptorV1>, StructuredError> {
    canonical_recipe_registry_v1(
        RecipeKindV1::ALL
            .iter()
            .copied()
            .map(recipe_descriptor_v1)
            .collect(),
    )
}

pub fn recipe_descriptor_digest_v1(kind: RecipeKindV1) -> Result<String, StructuredError> {
    catalog_digest(
        RECIPE_DESCRIPTOR_DIGEST_DOMAIN,
        &recipe_descriptor_v1(kind),
        "intent.recipe_registry.descriptor_digest",
    )
}

pub fn recipe_registry_digest_v1() -> Result<String, StructuredError> {
    let registry = recipe_registry_v1()?;
    catalog_digest(
        RECIPE_REGISTRY_DIGEST_DOMAIN,
        registry.as_slice(),
        "intent.recipe_registry.digest",
    )
}

pub(super) fn private_study_room_descriptor() -> RecipeDescriptorV1 {
    recipe_descriptor_v1(RecipeKindV1::PrivateStudyRoomV1)
}

pub(super) fn registered_recipe_kind_v1(
    id: &str,
    version: u32,
) -> Result<RecipeKindV1, StructuredError> {
    recipe_registry_v1()?
        .into_iter()
        .find(|descriptor| descriptor.id == id && descriptor.version == version)
        .map(|descriptor| descriptor.kind)
        .ok_or_else(|| {
            registry_error(
                "INTENT_RECIPE_NOT_REGISTERED",
                "intent.recipe_registry.selection",
                format!("Recipe {id} version {version} is not registered"),
                "Select an exact recipe id and version from the closed registry",
            )
        })
}

pub(super) fn canonical_recipe_registry_v1(
    mut descriptors: Vec<RecipeDescriptorV1>,
) -> Result<Vec<RecipeDescriptorV1>, StructuredError> {
    let mut kinds = BTreeSet::new();
    let mut identities = BTreeSet::new();
    for descriptor in &descriptors {
        if !kinds.insert(descriptor.kind) {
            return Err(registry_error(
                "DUPLICATE_INTENT_RECIPE_KIND",
                "intent.recipe_registry.kind",
                format!(
                    "Recipe kind {:?} is registered more than once",
                    descriptor.kind
                ),
                "Register every closed recipe kind exactly once",
            ));
        }
        if !identities.insert((descriptor.id, descriptor.version)) {
            return Err(registry_error(
                "DUPLICATE_INTENT_RECIPE_IDENTITY",
                "intent.recipe_registry.identity",
                format!(
                    "Recipe {} version {} is registered more than once",
                    descriptor.id, descriptor.version
                ),
                "Use one descriptor for each recipe id and version",
            ));
        }
        if descriptor != &recipe_descriptor_v1(descriptor.kind) {
            return Err(registry_error(
                "INTENT_RECIPE_DESCRIPTOR_MISMATCH",
                "intent.recipe_registry.descriptor",
                format!(
                    "Recipe kind {:?} does not match its pinned descriptor",
                    descriptor.kind
                ),
                "Use the exact descriptor pinned by the closed recipe kind",
            ));
        }
        if descriptor.min_requirements > descriptor.max_requirements
            || descriptor.max_requirements > MAX_COMPILED_REQUIREMENTS
        {
            return Err(registry_error(
                "INTENT_RECIPE_REQUIREMENT_BOUNDS_INVALID",
                "intent.recipe_registry.requirement_bounds",
                format!(
                    "Recipe {} version {} has invalid requirement bounds {}..={}",
                    descriptor.id,
                    descriptor.version,
                    descriptor.min_requirements,
                    descriptor.max_requirements
                ),
                format!("Pin ordered bounds within the global maximum {MAX_COMPILED_REQUIREMENTS}"),
            ));
        }
        if descriptor.extractor_revision == 0
            || descriptor.normalizer_revision == 0
            || descriptor.compiler_revision == 0
            || descriptor.simulator_revision == 0
        {
            return Err(registry_error(
                "INTENT_RECIPE_REVISION_INVALID",
                "intent.recipe_registry.revisions",
                format!(
                    "Recipe {} version {} contains a zero component revision",
                    descriptor.id, descriptor.version
                ),
                "Pin every recipe component to a positive revision",
            ));
        }
    }
    let expected_kinds = RecipeKindV1::ALL.iter().copied().collect::<BTreeSet<_>>();
    if kinds != expected_kinds {
        return Err(registry_error(
            "INTENT_RECIPE_REGISTRATION_INCOMPLETE",
            "intent.recipe_registry",
            "The closed recipe registry does not contain every recipe kind",
            "Register every RecipeKindV1 exactly once",
        ));
    }
    descriptors.sort_by(|left, right| {
        (left.id, left.version, left.kind).cmp(&(right.id, right.version, right.kind))
    });
    Ok(descriptors)
}

fn catalog_digest(
    domain: &'static str,
    value: &(impl Serialize + ?Sized),
    location: &str,
) -> Result<String, StructuredError> {
    let envelope = CatalogDigestEnvelope { domain, value };
    let bytes = serde_json::to_vec(&envelope).map_err(|error| {
        registry_error(
            "INTENT_RECIPE_REGISTRY_SERIALIZATION_FAILED",
            location,
            "A deterministic recipe registry artifact could not be serialized",
            error.to_string(),
        )
    })?;
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    Ok(output)
}

fn registry_error(
    code: impl Into<String>,
    location: impl Into<String>,
    message: impl Into<String>,
    hint: impl Into<String>,
) -> StructuredError {
    StructuredError::new(code, location, message, hint)
}
