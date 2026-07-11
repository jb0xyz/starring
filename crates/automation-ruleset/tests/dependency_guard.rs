#[test]
fn automation_core_does_not_depend_on_automation_ruleset() {
    let manifest = include_str!("../../automation-core/Cargo.toml");
    assert!(
        !manifest.contains("automation-ruleset"),
        "automation-core must not depend on automation-ruleset (cyclic dependency)"
    );
}
