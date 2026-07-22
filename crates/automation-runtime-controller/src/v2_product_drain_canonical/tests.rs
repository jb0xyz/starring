use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    BindingRevision, DeploymentId, DeploymentRevision, InstallationId, RuntimeDeploymentTargetV1,
    TenantId,
};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;

use super::{
    validate_pair, wire, RuntimeCanonicalProductDrainV2, RuntimeProductDrainCanonicalErrorV2,
    RuntimeProductDrainCanonicalFieldV2, RuntimeProductDrainCanonicalRootV2,
    RuntimeProductDrainCorrelationV2, DRAIN_INTENT_MAX_OCTETS, PRODUCT_MUTATION_MAX_OCTETS,
};
use crate::v2_digest::{drain_intent_digest_v2, product_mutation_digest_v2};
use crate::{
    RuntimeDeploymentScopeV1, RuntimeDrainIntentDigestV2, RuntimeDrainIntentIdV2,
    RuntimeProductMutationDigestV2, RuntimeProductMutationKindV2, RuntimeProductMutationPreimageV2,
    RuntimeProductOperationIdV2, RuntimeProductSemanticRequestDigestV2, RuntimeServingSlotV2,
};

const PRODUCT_OPERATION_ID: &str = "00112233445566778899aabbccddeeff";
const DRAIN_INTENT_ID: &str = "ffeeddccbbaa99887766554433221100";
const GUILD_ID: u64 = 9_223_372_036_854_775_808;
const PRODUCT_DIGEST: &str = "e35c1116d5bee2949184cceff540ee2575ac389461270f96f525ccd9c193166d";
const DRAIN_DIGEST: &str = "edf1671e7c1395205cae7962d6cf043610c51b5ed49b2d4528d72351bed287fc";

fn scope() -> RuntimeDeploymentScopeV1 {
    RuntimeDeploymentScopeV1 {
        tenant_id: TenantId::parse("tenant:1").unwrap(),
        installation_id: InstallationId::parse("installation:1").unwrap(),
        deployment_id: DeploymentId::parse("deployment:1").unwrap(),
    }
}

fn target(guild_id: u64) -> RuntimeDeploymentTargetV1 {
    RuntimeDeploymentTargetV1 {
        guild_id: GuildId(guild_id),
        ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
        version: RuleSetVersionId::FIRST,
        content_hash: RuleSetContentHash::parse_hex(&"b".repeat(64)).unwrap(),
        binding_revision: BindingRevision::new(3).unwrap(),
        binding_fingerprint: ResourceBindingFingerprint::parse(&"a".repeat(64)).unwrap(),
    }
}

fn product(mutation_kind: RuntimeProductMutationKindV2) -> RuntimeProductMutationPreimageV2 {
    let expected_target = target(GUILD_ID);
    RuntimeProductMutationPreimageV2 {
        operation_id: RuntimeProductOperationIdV2::parse(PRODUCT_OPERATION_ID).unwrap(),
        scope: scope(),
        expected_revision: DeploymentRevision::new(11).unwrap(),
        slot: RuntimeServingSlotV2::from_target(&expected_target),
        expected_target,
        mutation_kind,
        product_semantic_request_digest: RuntimeProductSemanticRequestDigestV2::parse(
            "c".repeat(64),
        )
        .unwrap(),
    }
}

fn canonical() -> RuntimeCanonicalProductDrainV2 {
    RuntimeCanonicalProductDrainV2::new(
        product(RuntimeProductMutationKindV2::AuthorityChange),
        RuntimeDrainIntentIdV2::parse(DRAIN_INTENT_ID).unwrap(),
    )
    .unwrap()
}

fn expected_product_json() -> String {
    format!(
        concat!(
            "{{\"format_version\":2,\"operation_id\":\"{}\",",
            "\"scope\":{{\"tenant_id\":\"tenant:1\",\"installation_id\":",
            "\"installation:1\",\"deployment_id\":\"deployment:1\"}},",
            "\"expected_revision\":11,\"slot\":{{\"guild_id\":\"9223372036854775808\",",
            "\"ruleset_key\":\"studyroom\"}},\"expected_target\":{{\"guild_id\":",
            "\"9223372036854775808\",\"ruleset_key\":\"studyroom\",\"version\":1,",
            "\"content_hash\":\"{}\",\"binding_revision\":3,",
            "\"binding_fingerprint\":\"{}\"}},\"mutation_kind\":",
            "\"authority_change\",\"product_semantic_request_digest\":\"{}\"}}"
        ),
        PRODUCT_OPERATION_ID,
        "b".repeat(64),
        "a".repeat(64),
        "c".repeat(64),
    )
}

fn expected_drain_json() -> String {
    format!(
        concat!(
            "{{\"format_version\":2,\"key\":{{\"intent_id\":\"{}\",",
            "\"product_operation_id\":\"{}\",",
            "\"product_mutation_digest\":\"{}\",",
            "\"scope\":{{\"tenant_id\":\"tenant:1\",\"installation_id\":",
            "\"installation:1\",\"deployment_id\":\"deployment:1\"}},",
            "\"expected_revision\":11,\"slot\":{{\"guild_id\":\"9223372036854775808\",",
            "\"ruleset_key\":\"studyroom\"}},\"expected_target\":{{\"guild_id\":",
            "\"9223372036854775808\",\"ruleset_key\":\"studyroom\",\"version\":1,",
            "\"content_hash\":\"{}\",\"binding_revision\":3,",
            "\"binding_fingerprint\":\"{}\"}},\"mutation_kind\":",
            "\"authority_change\"}}}}"
        ),
        DRAIN_INTENT_ID,
        PRODUCT_OPERATION_ID,
        PRODUCT_DIGEST,
        "b".repeat(64),
        "a".repeat(64),
    )
}

fn replace_once(bytes: &[u8], from: &str, to: &str) -> Vec<u8> {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .replacen(from, to, 1)
        .into_bytes()
}

fn assert_product_decode_error(bytes: &[u8], expected: RuntimeProductDrainCanonicalErrorV2) {
    assert_eq!(wire::decode_product_mutation(bytes), Err(expected));
}

fn assert_drain_decode_error(bytes: &[u8], expected: RuntimeProductDrainCanonicalErrorV2) {
    assert_eq!(wire::decode_drain_intent(bytes), Err(expected));
}

#[test]
fn product_and_drain_roots_match_the_exact_byte_and_digest_goldens() {
    let canonical = canonical();

    assert_eq!(
        canonical.product_mutation_request_bytes(),
        expected_product_json().as_bytes()
    );
    assert_eq!(canonical.product_mutation_request_bytes().len(), 679);
    assert_eq!(canonical.product_mutation_digest().as_str(), PRODUCT_DIGEST);
    assert_eq!(
        canonical.drain_intent_request_bytes(),
        expected_drain_json().as_bytes()
    );
    assert_eq!(canonical.drain_intent_request_bytes().len(), 734);
    assert_eq!(canonical.drain_intent_digest().as_str(), DRAIN_DIGEST);
    assert_eq!(
        canonical.drain_preimage().key.product_mutation_digest,
        canonical.product_mutation_digest().clone()
    );
}

#[test]
fn persisted_roots_reconstruct_only_after_exact_digest_and_pair_validation() {
    let canonical = canonical();
    let reconstructed = RuntimeCanonicalProductDrainV2::from_persisted(
        canonical.product_mutation_request_bytes(),
        canonical.product_mutation_digest(),
        canonical.drain_intent_request_bytes(),
        canonical.drain_intent_digest(),
    )
    .unwrap();

    assert_eq!(reconstructed, canonical);

    let wrong_product = RuntimeProductMutationDigestV2::parse("0".repeat(64)).unwrap();
    assert_eq!(
        RuntimeCanonicalProductDrainV2::from_persisted(
            canonical.product_mutation_request_bytes(),
            &wrong_product,
            canonical.drain_intent_request_bytes(),
            canonical.drain_intent_digest(),
        ),
        Err(
            RuntimeProductDrainCanonicalErrorV2::PersistedDigestMismatch {
                root: RuntimeProductDrainCanonicalRootV2::ProductMutation,
            }
        )
    );

    let wrong_drain = RuntimeDrainIntentDigestV2::parse("0".repeat(64)).unwrap();
    assert_eq!(
        RuntimeCanonicalProductDrainV2::from_persisted(
            canonical.product_mutation_request_bytes(),
            canonical.product_mutation_digest(),
            canonical.drain_intent_request_bytes(),
            &wrong_drain,
        ),
        Err(
            RuntimeProductDrainCanonicalErrorV2::PersistedDigestMismatch {
                root: RuntimeProductDrainCanonicalRootV2::DrainIntent,
            }
        )
    );
}

#[test]
fn all_product_mutation_tags_are_explicit_and_stable() {
    for (kind, tag) in [
        (RuntimeProductMutationKindV2::Apply, "apply"),
        (RuntimeProductMutationKindV2::Supersede, "supersede"),
        (RuntimeProductMutationKindV2::Cancel, "cancel"),
        (
            RuntimeProductMutationKindV2::AuthorityChange,
            "authority_change",
        ),
        (RuntimeProductMutationKindV2::Teardown, "teardown"),
    ] {
        let canonical = RuntimeCanonicalProductDrainV2::new(
            product(kind),
            RuntimeDrainIntentIdV2::parse(DRAIN_INTENT_ID).unwrap(),
        )
        .unwrap();
        let needle = format!("\"mutation_kind\":\"{tag}\"");

        assert!(
            String::from_utf8_lossy(canonical.product_mutation_request_bytes()).contains(&needle)
        );
        assert!(String::from_utf8_lossy(canonical.drain_intent_request_bytes()).contains(&needle));
        assert!(String::from_utf8_lossy(canonical.drain_intent_request_bytes()).contains(&needle));
    }
}

#[test]
fn product_decoder_rejects_noncanonical_and_structurally_invalid_json() {
    let canonical = canonical();
    let bytes = canonical.product_mutation_request_bytes();
    let decoding = RuntimeProductDrainCanonicalErrorV2::Decoding {
        root: RuntimeProductDrainCanonicalRootV2::ProductMutation,
    };
    let noncanonical = RuntimeProductDrainCanonicalErrorV2::NonCanonicalEncoding {
        root: RuntimeProductDrainCanonicalRootV2::ProductMutation,
    };

    for hostile in [
        replace_once(
            bytes,
            "\"format_version\":2,",
            "\"format_version\":2,\"format_version\":2,",
        ),
        replace_once(
            bytes,
            "\"operation_id\":\"00112233445566778899aabbccddeeff\",",
            "",
        ),
        replace_once(
            bytes,
            "\"deployment_id\":\"deployment:1\"}",
            "\"deployment_id\":\"deployment:1\",\"unexpected\":true}",
        ),
        replace_once(
            bytes,
            "\"tenant_id\":\"tenant:1\",",
            "\"tenant_id\":\"tenant:1\",\"tenant_id\":\"tenant:1\",",
        ),
        replace_once(bytes, "\"tenant_id\":\"tenant:1\",", ""),
        replace_once(
            bytes,
            "\"ruleset_key\":\"studyroom\"}",
            "\"ruleset_key\":\"studyroom\",\"unexpected\":true}",
        ),
        replace_once(
            bytes,
            "\"slot\":{\"guild_id\":\"9223372036854775808\",",
            concat!(
                "\"slot\":{\"guild_id\":\"9223372036854775808\",",
                "\"guild_id\":\"9223372036854775808\","
            ),
        ),
        replace_once(
            bytes,
            "\"slot\":{\"guild_id\":\"9223372036854775808\",",
            "\"slot\":{",
        ),
        replace_once(
            bytes,
            &format!("\"binding_fingerprint\":\"{}\"}}", "a".repeat(64)),
            &format!(
                "\"binding_fingerprint\":\"{}\",\"unexpected\":true}}",
                "a".repeat(64)
            ),
        ),
        replace_once(
            bytes,
            "\"expected_target\":{\"guild_id\":\"9223372036854775808\",",
            concat!(
                "\"expected_target\":{\"guild_id\":\"9223372036854775808\",",
                "\"guild_id\":\"9223372036854775808\","
            ),
        ),
        replace_once(
            bytes,
            "\"expected_target\":{\"guild_id\":\"9223372036854775808\",",
            "\"expected_target\":{",
        ),
    ] {
        assert_product_decode_error(&hostile, decoding);
    }

    let mut root_unknown = String::from_utf8(bytes.to_vec()).unwrap();
    assert_eq!(root_unknown.pop(), Some('}'));
    root_unknown.push_str(",\"unexpected\":true}");
    assert_product_decode_error(root_unknown.as_bytes(), decoding);

    let reordered = replace_once(
        bytes,
        &format!("{{\"format_version\":2,\"operation_id\":\"{PRODUCT_OPERATION_ID}\""),
        &format!("{{\"operation_id\":\"{PRODUCT_OPERATION_ID}\",\"format_version\":2"),
    );
    assert_product_decode_error(&reordered, noncanonical);

    let mut whitespace = vec![b' '];
    whitespace.extend_from_slice(bytes);
    assert_product_decode_error(&whitespace, noncanonical);

    let mut trailing_whitespace = bytes.to_vec();
    trailing_whitespace.push(b' ');
    assert_product_decode_error(&trailing_whitespace, noncanonical);

    let alternate_id = format!("\\u0030{}", &PRODUCT_OPERATION_ID[1..]);
    let alternate_escape = replace_once(bytes, PRODUCT_OPERATION_ID, &alternate_id);
    assert_product_decode_error(&alternate_escape, noncanonical);

    let mut trailing = bytes.to_vec();
    trailing.extend_from_slice(b"false");
    assert_product_decode_error(&trailing, decoding);

    let mut invalid_utf8 = bytes.to_vec();
    invalid_utf8[0] = 0xff;
    assert_product_decode_error(&invalid_utf8, decoding);

    let wrong_version = replace_once(bytes, "\"format_version\":2", "\"format_version\":3");
    assert_product_decode_error(
        &wrong_version,
        RuntimeProductDrainCanonicalErrorV2::UnsupportedFormatVersion {
            root: RuntimeProductDrainCanonicalRootV2::ProductMutation,
        },
    );
}

#[test]
fn drain_decoder_rejects_noncanonical_and_nested_unknown_json() {
    let canonical = canonical();
    let bytes = canonical.drain_intent_request_bytes();
    let decoding = RuntimeProductDrainCanonicalErrorV2::Decoding {
        root: RuntimeProductDrainCanonicalRootV2::DrainIntent,
    };
    let noncanonical = RuntimeProductDrainCanonicalErrorV2::NonCanonicalEncoding {
        root: RuntimeProductDrainCanonicalRootV2::DrainIntent,
    };

    for hostile in [
        replace_once(
            bytes,
            "\"format_version\":2,",
            "\"format_version\":2,\"format_version\":2,",
        ),
        replace_once(bytes, &format!("\"intent_id\":\"{DRAIN_INTENT_ID}\","), ""),
        replace_once(
            bytes,
            &format!("\"intent_id\":\"{DRAIN_INTENT_ID}\","),
            &format!("\"intent_id\":\"{DRAIN_INTENT_ID}\",\"intent_id\":\"{DRAIN_INTENT_ID}\","),
        ),
        replace_once(
            bytes,
            "\"deployment_id\":\"deployment:1\"}",
            "\"deployment_id\":\"deployment:1\",\"unexpected\":true}",
        ),
        replace_once(bytes, "}}", ",\"unexpected\":true}}"),
        replace_once(bytes, "}", ",\"unexpected\":true}"),
    ] {
        assert_drain_decode_error(&hostile, decoding);
    }

    assert_drain_decode_error(b"{\"format_version\":2}", decoding);

    let mut root_unknown = String::from_utf8(bytes.to_vec()).unwrap();
    assert_eq!(root_unknown.pop(), Some('}'));
    root_unknown.push_str(",\"unexpected\":true}");
    assert_drain_decode_error(root_unknown.as_bytes(), decoding);

    let reordered = replace_once(
        bytes,
        &format!("{{\"format_version\":2,\"key\":{{\"intent_id\":\"{DRAIN_INTENT_ID}\""),
        &format!("{{\"key\":{{\"intent_id\":\"{DRAIN_INTENT_ID}\""),
    );
    let reordered = replace_once(&reordered, "}}", "},\"format_version\":2}");
    assert_drain_decode_error(&reordered, noncanonical);

    let mut whitespace = bytes.to_vec();
    whitespace.push(b'\n');
    assert_drain_decode_error(&whitespace, noncanonical);

    let alternate_id = format!("\\u0066{}", &DRAIN_INTENT_ID[1..]);
    let alternate_escape = replace_once(bytes, DRAIN_INTENT_ID, &alternate_id);
    assert_drain_decode_error(&alternate_escape, noncanonical);

    let wrong_version = replace_once(bytes, "\"format_version\":2", "\"format_version\":3");
    assert_drain_decode_error(
        &wrong_version,
        RuntimeProductDrainCanonicalErrorV2::UnsupportedFormatVersion {
            root: RuntimeProductDrainCanonicalRootV2::DrainIntent,
        },
    );

    let mut trailing = bytes.to_vec();
    trailing.extend_from_slice(b"null");
    assert_drain_decode_error(&trailing, decoding);
}

#[test]
fn decoders_reject_invalid_fixed_fields_with_stable_field_errors() {
    let canonical = canonical();
    let product_bytes = canonical.product_mutation_request_bytes();
    let product_root = RuntimeProductDrainCanonicalRootV2::ProductMutation;

    for (hostile, field) in [
        (
            replace_once(product_bytes, PRODUCT_OPERATION_ID, &"A".repeat(32)),
            RuntimeProductDrainCanonicalFieldV2::OperationId,
        ),
        (
            replace_once(product_bytes, "tenant:1", "tenant 1"),
            RuntimeProductDrainCanonicalFieldV2::TenantId,
        ),
        (
            replace_once(
                product_bytes,
                "\"ruleset_key\":\"studyroom\"",
                "\"ruleset_key\":\"bad key\"",
            ),
            RuntimeProductDrainCanonicalFieldV2::SlotRuleSetKey,
        ),
        (
            replace_once(product_bytes, "\"version\":1", "\"version\":0"),
            RuntimeProductDrainCanonicalFieldV2::TargetVersion,
        ),
        (
            replace_once(product_bytes, &"b".repeat(64), &"B".repeat(64)),
            RuntimeProductDrainCanonicalFieldV2::TargetContentHash,
        ),
        (
            replace_once(
                product_bytes,
                "\"binding_revision\":3",
                "\"binding_revision\":0",
            ),
            RuntimeProductDrainCanonicalFieldV2::TargetBindingRevision,
        ),
        (
            replace_once(product_bytes, &"a".repeat(64), &"A".repeat(64)),
            RuntimeProductDrainCanonicalFieldV2::TargetBindingFingerprint,
        ),
        (
            replace_once(product_bytes, "authority_change", "unknown"),
            RuntimeProductDrainCanonicalFieldV2::MutationKind,
        ),
        (
            replace_once(product_bytes, &"c".repeat(64), &"C".repeat(64)),
            RuntimeProductDrainCanonicalFieldV2::ProductSemanticRequestDigest,
        ),
    ] {
        assert_product_decode_error(
            &hostile,
            RuntimeProductDrainCanonicalErrorV2::InvalidField {
                root: product_root,
                field,
            },
        );
    }

    let drain_bytes = canonical.drain_intent_request_bytes();
    let drain_root = RuntimeProductDrainCanonicalRootV2::DrainIntent;
    for (hostile, field) in [
        (
            replace_once(drain_bytes, DRAIN_INTENT_ID, &"F".repeat(32)),
            RuntimeProductDrainCanonicalFieldV2::IntentId,
        ),
        (
            replace_once(drain_bytes, PRODUCT_DIGEST, &"E".repeat(64)),
            RuntimeProductDrainCanonicalFieldV2::ProductMutationDigest,
        ),
    ] {
        assert_drain_decode_error(
            &hostile,
            RuntimeProductDrainCanonicalErrorV2::InvalidField {
                root: drain_root,
                field,
            },
        );
    }
}

#[test]
fn root_size_limits_are_checked_before_json_decoding() {
    assert_product_decode_error(
        &vec![b' '; PRODUCT_MUTATION_MAX_OCTETS + 1],
        RuntimeProductDrainCanonicalErrorV2::PayloadTooLarge {
            root: RuntimeProductDrainCanonicalRootV2::ProductMutation,
        },
    );
    assert_drain_decode_error(
        &vec![b' '; DRAIN_INTENT_MAX_OCTETS + 1],
        RuntimeProductDrainCanonicalErrorV2::PayloadTooLarge {
            root: RuntimeProductDrainCanonicalRootV2::DrainIntent,
        },
    );
    assert_product_decode_error(
        &vec![b' '; PRODUCT_MUTATION_MAX_OCTETS],
        RuntimeProductDrainCanonicalErrorV2::Decoding {
            root: RuntimeProductDrainCanonicalRootV2::ProductMutation,
        },
    );
    assert_drain_decode_error(
        &vec![b' '; DRAIN_INTENT_MAX_OCTETS],
        RuntimeProductDrainCanonicalErrorV2::Decoding {
            root: RuntimeProductDrainCanonicalRootV2::DrainIntent,
        },
    );
}

#[test]
fn snowflakes_use_canonical_full_u64_json_strings() {
    for guild_id in [1, i64::MAX as u64 + 1, u64::MAX] {
        let expected_target = target(guild_id);
        let mut product = product(RuntimeProductMutationKindV2::Apply);
        product.slot = RuntimeServingSlotV2::from_target(&expected_target);
        product.expected_target = expected_target;
        let canonical = RuntimeCanonicalProductDrainV2::new(
            product,
            RuntimeDrainIntentIdV2::parse(DRAIN_INTENT_ID).unwrap(),
        )
        .unwrap();
        let needle = format!("\"guild_id\":\"{guild_id}\"");

        assert!(
            String::from_utf8_lossy(canonical.product_mutation_request_bytes()).contains(&needle)
        );
    }

    let canonical = canonical();
    for invalid in ["0", "01", "+1", "18446744073709551616"] {
        let hostile = String::from_utf8(canonical.product_mutation_request_bytes().to_vec())
            .unwrap()
            .replace("9223372036854775808", invalid)
            .into_bytes();
        assert!(matches!(
            wire::decode_product_mutation(&hostile),
            Err(RuntimeProductDrainCanonicalErrorV2::CanonicalValue { .. })
        ));
    }

    let number = String::from_utf8(canonical.product_mutation_request_bytes().to_vec())
        .unwrap()
        .replace(
            "\"guild_id\":\"9223372036854775808\"",
            "\"guild_id\":9223372036854775808",
        )
        .into_bytes();
    assert_product_decode_error(
        &number,
        RuntimeProductDrainCanonicalErrorV2::Decoding {
            root: RuntimeProductDrainCanonicalRootV2::ProductMutation,
        },
    );
}

#[test]
fn persistence_u64_bounds_apply_on_encode_and_decode() {
    let mut maximum = product(RuntimeProductMutationKindV2::Apply);
    maximum.expected_revision = DeploymentRevision::new(i64::MAX as u64).unwrap();
    maximum.expected_target.binding_revision = BindingRevision::new(i64::MAX as u64).unwrap();
    assert!(RuntimeCanonicalProductDrainV2::new(
        maximum,
        RuntimeDrainIntentIdV2::parse(DRAIN_INTENT_ID).unwrap(),
    )
    .is_ok());

    let mut overflow = product(RuntimeProductMutationKindV2::Apply);
    overflow.expected_revision = DeploymentRevision::new(i64::MAX as u64 + 1).unwrap();
    assert!(matches!(
        RuntimeCanonicalProductDrainV2::new(
            overflow,
            RuntimeDrainIntentIdV2::parse(DRAIN_INTENT_ID).unwrap(),
        ),
        Err(RuntimeProductDrainCanonicalErrorV2::CanonicalValue { .. })
    ));

    let mut binding_overflow = product(RuntimeProductMutationKindV2::Apply);
    binding_overflow.expected_target.binding_revision =
        BindingRevision::new(i64::MAX as u64 + 1).unwrap();
    assert!(matches!(
        RuntimeCanonicalProductDrainV2::new(
            binding_overflow,
            RuntimeDrainIntentIdV2::parse(DRAIN_INTENT_ID).unwrap(),
        ),
        Err(RuntimeProductDrainCanonicalErrorV2::CanonicalValue { .. })
    ));

    let canonical = canonical();
    let zero_revision = replace_once(
        canonical.product_mutation_request_bytes(),
        "\"expected_revision\":11",
        "\"expected_revision\":0",
    );
    assert!(matches!(
        wire::decode_product_mutation(&zero_revision),
        Err(RuntimeProductDrainCanonicalErrorV2::InvalidField { .. })
    ));
    let numeric_overflow = replace_once(
        canonical.product_mutation_request_bytes(),
        "\"expected_revision\":11",
        "\"expected_revision\":9223372036854775808",
    );
    assert!(matches!(
        wire::decode_product_mutation(&numeric_overflow),
        Err(RuntimeProductDrainCanonicalErrorV2::CanonicalValue { .. })
    ));
}

#[test]
fn each_root_rejects_an_internal_slot_target_mismatch() {
    let mut mismatched = product(RuntimeProductMutationKindV2::Apply);
    mismatched.slot.guild_id = GuildId(9);
    assert_eq!(
        RuntimeCanonicalProductDrainV2::new(
            mismatched,
            RuntimeDrainIntentIdV2::parse(DRAIN_INTENT_ID).unwrap(),
        ),
        Err(RuntimeProductDrainCanonicalErrorV2::SlotTargetMismatch {
            root: RuntimeProductDrainCanonicalRootV2::ProductMutation,
        })
    );

    let canonical = canonical();
    let product_hostile = replace_once(
        canonical.product_mutation_request_bytes(),
        "\"guild_id\":\"9223372036854775808\"",
        "\"guild_id\":\"9\"",
    );
    assert_product_decode_error(
        &product_hostile,
        RuntimeProductDrainCanonicalErrorV2::SlotTargetMismatch {
            root: RuntimeProductDrainCanonicalRootV2::ProductMutation,
        },
    );

    let drain_hostile = replace_once(
        canonical.drain_intent_request_bytes(),
        "\"guild_id\":\"9223372036854775808\"",
        "\"guild_id\":\"9\"",
    );
    assert_drain_decode_error(
        &drain_hostile,
        RuntimeProductDrainCanonicalErrorV2::SlotTargetMismatch {
            root: RuntimeProductDrainCanonicalRootV2::DrainIntent,
        },
    );
}

fn correlation_error(
    product: &RuntimeProductMutationPreimageV2,
    product_digest: &RuntimeProductMutationDigestV2,
    drain: &crate::RuntimeDrainIntentPreimageV2,
) -> RuntimeProductDrainCanonicalErrorV2 {
    validate_pair(product, product_digest, drain).unwrap_err()
}

#[test]
fn pair_validation_rejects_each_cross_root_mismatch() {
    let canonical = canonical();
    let product = canonical.product_preimage();
    let product_digest = canonical.product_mutation_digest();

    let mut drain = canonical.drain_preimage().clone();
    drain.key.product_operation_id =
        RuntimeProductOperationIdV2::parse("11112233445566778899aabbccddeeff").unwrap();
    assert_eq!(
        correlation_error(product, product_digest, &drain),
        RuntimeProductDrainCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeProductDrainCorrelationV2::ProductOperationId,
        }
    );

    let mut drain = canonical.drain_preimage().clone();
    drain.key.product_mutation_digest =
        RuntimeProductMutationDigestV2::parse("0".repeat(64)).unwrap();
    assert_eq!(
        correlation_error(product, product_digest, &drain),
        RuntimeProductDrainCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeProductDrainCorrelationV2::ProductMutationDigest,
        }
    );

    let mut drain = canonical.drain_preimage().clone();
    drain.key.scope.deployment_id = DeploymentId::parse("deployment:2").unwrap();
    assert_eq!(
        correlation_error(product, product_digest, &drain),
        RuntimeProductDrainCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeProductDrainCorrelationV2::Scope,
        }
    );

    let mut drain = canonical.drain_preimage().clone();
    drain.key.expected_revision = DeploymentRevision::new(12).unwrap();
    assert_eq!(
        correlation_error(product, product_digest, &drain),
        RuntimeProductDrainCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeProductDrainCorrelationV2::ExpectedRevision,
        }
    );

    let mut drain = canonical.drain_preimage().clone();
    drain.key.slot.guild_id = GuildId(9);
    drain.key.expected_target.guild_id = GuildId(9);
    assert_eq!(
        correlation_error(product, product_digest, &drain),
        RuntimeProductDrainCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeProductDrainCorrelationV2::Slot,
        }
    );

    let mut drain = canonical.drain_preimage().clone();
    drain.key.expected_target.version = RuleSetVersionId::new(2).unwrap();
    assert_eq!(
        correlation_error(product, product_digest, &drain),
        RuntimeProductDrainCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeProductDrainCorrelationV2::ExpectedTarget,
        }
    );

    let mut drain = canonical.drain_preimage().clone();
    drain.key.mutation_kind = RuntimeProductMutationKindV2::Teardown;
    assert_eq!(
        correlation_error(product, product_digest, &drain),
        RuntimeProductDrainCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeProductDrainCorrelationV2::MutationKind,
        }
    );
}

#[test]
fn persisted_path_rejects_a_canonical_but_cross_mismatched_drain_root() {
    let canonical = canonical();
    let mut drain = canonical.drain_preimage().clone();
    drain.key.expected_revision = DeploymentRevision::new(12).unwrap();
    let drain_bytes = wire::encode_drain_intent(&drain).unwrap();
    let drain_digest = drain_intent_digest_v2(&drain_bytes);

    assert_eq!(
        RuntimeCanonicalProductDrainV2::from_persisted(
            canonical.product_mutation_request_bytes(),
            canonical.product_mutation_digest(),
            &drain_bytes,
            &drain_digest,
        ),
        Err(RuntimeProductDrainCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeProductDrainCorrelationV2::ExpectedRevision,
        })
    );
}

#[test]
fn changing_each_product_or_drain_identity_changes_its_typed_digest() {
    let canonical = canonical();
    let base_product = canonical.product_preimage().clone();
    let mut product_variants = Vec::new();

    let mut value = base_product.clone();
    value.operation_id =
        RuntimeProductOperationIdV2::parse("11112233445566778899aabbccddeeff").unwrap();
    product_variants.push(value);
    let mut value = base_product.clone();
    value.scope.deployment_id = DeploymentId::parse("deployment:2").unwrap();
    product_variants.push(value);
    let mut value = base_product.clone();
    value.expected_revision = DeploymentRevision::new(12).unwrap();
    product_variants.push(value);
    let mut value = base_product.clone();
    value.expected_target.version = RuleSetVersionId::new(2).unwrap();
    product_variants.push(value);
    let mut value = base_product.clone();
    value.slot.guild_id = GuildId(9);
    value.expected_target.guild_id = GuildId(9);
    product_variants.push(value);
    let mut value = base_product.clone();
    value.mutation_kind = RuntimeProductMutationKindV2::Teardown;
    product_variants.push(value);
    let mut value = base_product;
    value.product_semantic_request_digest =
        RuntimeProductSemanticRequestDigestV2::parse("d".repeat(64)).unwrap();
    product_variants.push(value);

    for value in product_variants {
        let bytes = wire::encode_product_mutation(&value).unwrap();
        assert_ne!(
            product_mutation_digest_v2(&bytes),
            canonical.product_mutation_digest().clone()
        );
    }

    let base_drain = canonical.drain_preimage().clone();
    let mut drain_variants = Vec::new();
    let mut value = base_drain.clone();
    value.key.intent_id =
        RuntimeDrainIntentIdV2::parse("11112233445566778899aabbccddeeff").unwrap();
    drain_variants.push(value);
    let mut value = base_drain.clone();
    value.key.product_operation_id =
        RuntimeProductOperationIdV2::parse("11112233445566778899aabbccddeeff").unwrap();
    drain_variants.push(value);
    let mut value = base_drain.clone();
    value.key.product_mutation_digest =
        RuntimeProductMutationDigestV2::parse("0".repeat(64)).unwrap();
    drain_variants.push(value);
    let mut value = base_drain.clone();
    value.key.scope.deployment_id = DeploymentId::parse("deployment:2").unwrap();
    drain_variants.push(value);
    let mut value = base_drain.clone();
    value.key.expected_revision = DeploymentRevision::new(12).unwrap();
    drain_variants.push(value);
    let mut value = base_drain.clone();
    value.key.expected_target.version = RuleSetVersionId::new(2).unwrap();
    drain_variants.push(value);
    let mut value = base_drain.clone();
    value.key.slot.guild_id = GuildId(9);
    value.key.expected_target.guild_id = GuildId(9);
    drain_variants.push(value);
    let mut value = base_drain;
    value.key.mutation_kind = RuntimeProductMutationKindV2::Teardown;
    drain_variants.push(value);

    for value in drain_variants {
        let bytes = wire::encode_drain_intent(&value).unwrap();
        assert_ne!(
            drain_intent_digest_v2(&bytes),
            canonical.drain_intent_digest().clone()
        );
    }
}
