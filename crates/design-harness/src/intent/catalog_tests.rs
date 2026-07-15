use super::catalog::{
    canonical_recipe_registry_v1, recipe_descriptor_digest_v1, recipe_descriptor_v1,
    recipe_registry_digest_v1, recipe_registry_v1, RecipeKindV1, COMPILER_REVISION,
    MAX_COMPILED_REQUIREMENTS,
};
use super::model::{PRIVATE_STUDY_ROOM_RECIPE_ID, PRIVATE_STUDY_ROOM_RECIPE_VERSION};

#[test]
fn registry_is_complete_and_canonically_sorted() {
    let registry = recipe_registry_v1().expect("closed registry should be valid");
    assert_eq!(registry.len(), 1);
    assert!(registry.windows(2).all(|pair| {
        (pair[0].id, pair[0].version, pair[0].kind) < (pair[1].id, pair[1].version, pair[1].kind)
    }));
    assert_eq!(registry[0].kind, RecipeKindV1::PrivateStudyRoomV1);
}

#[test]
fn private_study_room_descriptor_pins_every_component_and_bound() {
    let descriptor = recipe_descriptor_v1(RecipeKindV1::PrivateStudyRoomV1);
    assert_eq!(descriptor.id, PRIVATE_STUDY_ROOM_RECIPE_ID);
    assert_eq!(descriptor.version, PRIVATE_STUDY_ROOM_RECIPE_VERSION);
    assert_eq!(descriptor.extractor_revision, 10);
    assert_eq!(descriptor.normalizer_revision, 6);
    assert_eq!(descriptor.compiler_revision, COMPILER_REVISION);
    assert_eq!(descriptor.simulator_revision, 1);
    assert_eq!(descriptor.min_requirements, 22);
    assert_eq!(descriptor.max_requirements, 26);
    assert!(descriptor.max_requirements <= MAX_COMPILED_REQUIREMENTS);
}

#[test]
fn selected_descriptor_and_full_registry_have_distinct_stable_digests() {
    let selected = recipe_descriptor_digest_v1(RecipeKindV1::PrivateStudyRoomV1)
        .expect("descriptor should hash");
    let registry = recipe_registry_digest_v1().expect("registry should hash");
    assert_eq!(selected.len(), 64);
    assert_eq!(registry.len(), 64);
    assert_ne!(selected, registry);
    assert_eq!(
        selected,
        "9a78514f99ec552a7d96f6affa2f486a3d65daad67ca13a9e06969d86380bbdc"
    );
    assert_eq!(
        registry,
        "2bec5c1e5e7cce0084e98716eeae362ae797402ab9c3f1b4459da60c44bc8650"
    );
    assert_eq!(
        selected,
        recipe_descriptor_digest_v1(RecipeKindV1::PrivateStudyRoomV1)
            .expect("repeat descriptor hash should be stable")
    );
    assert_eq!(
        registry,
        recipe_registry_digest_v1().expect("repeat registry hash should be stable")
    );
}

#[test]
fn duplicate_recipe_registration_is_rejected() {
    let descriptor = recipe_descriptor_v1(RecipeKindV1::PrivateStudyRoomV1);
    let error = canonical_recipe_registry_v1(vec![descriptor, descriptor]).unwrap_err();
    assert_eq!(error.code, "DUPLICATE_INTENT_RECIPE_KIND");
}

#[test]
fn missing_recipe_registration_is_rejected() {
    let error = canonical_recipe_registry_v1(Vec::new()).unwrap_err();
    assert_eq!(error.code, "INTENT_RECIPE_REGISTRATION_INCOMPLETE");
}

#[test]
fn changed_descriptor_registration_is_rejected() {
    let mut descriptor = recipe_descriptor_v1(RecipeKindV1::PrivateStudyRoomV1);
    descriptor.extractor_revision += 1;
    let error = canonical_recipe_registry_v1(vec![descriptor]).unwrap_err();
    assert_eq!(error.code, "INTENT_RECIPE_DESCRIPTOR_MISMATCH");
}
