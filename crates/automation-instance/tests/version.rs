use automation_instance::InstanceRuleSetVersion;

#[test]
fn version_one_is_valid() {
    assert_eq!(InstanceRuleSetVersion::new(1).unwrap().get(), 1);
}

#[test]
fn zero_json_is_rejected() {
    assert!(serde_json::from_str::<InstanceRuleSetVersion>("0").is_err());
}

#[test]
fn max_version_is_valid() {
    assert_eq!(
        InstanceRuleSetVersion::new(u32::MAX).unwrap().get(),
        u32::MAX
    );
}
