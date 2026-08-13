#[test]
fn effect_contract_stays_below_runtime_and_persistence_layers() {
    let manifest = include_str!("../Cargo.toml");
    for forbidden in [
        "automation-runtime =",
        "automation-stateful-runtime =",
        "sqlx =",
        "twilight-http =",
        "serde =",
        "serde_json =",
    ] {
        assert!(
            !manifest.contains(forbidden),
            "effect contract must not depend on `{forbidden}`"
        );
    }
}

#[test]
fn opaque_dispatch_exposes_no_digest_or_byte_shaped_constructor() {
    let source = include_str!("../src/dispatch.rs");
    for forbidden in [
        "pub fn from_canonical_v1",
        "pub fn from_bytes",
        "pub fn parse(value",
        "pub fn new(bytes",
        "pub fn new(digest",
    ] {
        assert!(
            !source.contains(forbidden),
            "opaque dispatch must not expose `{forbidden}`"
        );
    }
}

#[test]
fn prepared_dispatch_construction_is_test_only_until_exact_body_binding_exists() {
    let source = include_str!("../src/dispatch.rs");
    assert!(!source.contains("from_compiler_v1"));
    let lines = source.lines().collect::<Vec<_>>();
    for (index, line) in lines.iter().enumerate() {
        if line.contains("pub(crate) fn") {
            assert!(
                lines[..index]
                    .iter()
                    .rev()
                    .take(3)
                    .any(|candidate| candidate.trim() == "#[cfg(test)]"),
                "dispatch constructor must remain test-only: {line}"
            );
        }
    }
}
