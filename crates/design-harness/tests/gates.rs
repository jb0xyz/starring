mod support;

use automation_state::{ActionSpec, ChannelRef, CreatedRef, InteractionRule, TriggerSpec};
use design_harness::{dispatch_tool, Draft};
use futures::executor::block_on;
use serde_json::json;

#[test]
fn simulate_requires_validation_on_the_current_revision() {
    block_on(async {
        let mut draft = support::golden_draft().await;
        assert!(draft.summary().unresolved_references.is_empty());
        let revision = draft.draft_revision;

        let result = dispatch_tool(&mut draft, "simulate_draft", "{}").await;

        let failure = result.failure().unwrap();
        assert_eq!(failure.code, "DRAFT_NOT_VALIDATED");
        assert_eq!(failure.location, "draft.validation");
        assert!(failure.hint.contains("validate_draft"));
        assert_eq!(draft.draft_revision, revision);
        assert_eq!(draft.simulated_revision, None);
    });
}

#[test]
fn golden_studyroom_validates_and_simulates_through_core_run() {
    block_on(async {
        let mut draft = support::golden_draft().await;

        let validation = dispatch_tool(&mut draft, "validate_draft", "{}").await;
        assert!(validation.is_ok(), "{}", validation.as_json());
        assert_eq!(draft.validated_revision, Some(draft.draft_revision));

        let simulation = dispatch_tool(&mut draft, "simulate_draft", "{}").await;
        assert!(simulation.is_ok(), "{}", simulation.as_json());
        assert_eq!(draft.simulated_revision, Some(draft.draft_revision));
        assert_eq!(
            simulation.success_value().unwrap().change,
            "Golden StudyRoom trace passed"
        );
    });
}

#[test]
fn mutation_after_simulation_invalidates_both_gates() {
    block_on(async {
        let mut draft = support::golden_draft().await;
        assert!(dispatch_tool(&mut draft, "validate_draft", "{}")
            .await
            .is_ok());
        assert!(dispatch_tool(&mut draft, "simulate_draft", "{}")
            .await
            .is_ok());

        assert!(dispatch_tool(
            &mut draft,
            "add_panel",
            &json!({"key":"extra","channel":"study_hub","content":"Extra"}).to_string(),
        )
        .await
        .is_ok());

        assert_eq!(draft.validated_revision, None);
        assert_eq!(draft.simulated_revision, None);
        assert_eq!(
            dispatch_tool(&mut draft, "simulate_draft", "{}")
                .await
                .failure()
                .unwrap()
                .code,
            "DRAFT_NOT_VALIDATED"
        );
    });
}

#[test]
fn validation_errors_are_structured_without_rust_debug() {
    block_on(async {
        let mut draft = Draft::new();
        assert!(dispatch_tool(
            &mut draft,
            "add_panel",
            &json!({"key":"panel","channel":"study_hub","content":"Panel"}).to_string(),
        )
        .await
        .is_ok());
        assert!(dispatch_tool(
            &mut draft,
            "add_button",
            &json!({
                "panel_key":"panel",
                "label":"Open",
                "route":{"kind":"static","key":"missing_button"}
            })
            .to_string(),
        )
        .await
        .is_ok());
        draft.ruleset.rules.push(InteractionRule {
            key: "broken".to_string(),
            trigger: TriggerSpec::ButtonClick {
                component: "missing_button".to_string(),
            },
            actions: vec![ActionSpec::PostPanel {
                key: "message".to_string(),
                channel: ChannelRef::Created(CreatedRef {
                    created: "missing_channel".to_string(),
                }),
                content: "hello".to_string(),
                buttons: vec![],
            }],
        });

        let result = dispatch_tool(&mut draft, "validate_draft", "{}").await;

        let failure = result.failure().unwrap();
        assert_eq!(failure.code, "UNRESOLVED_CREATED_REFERENCE");
        assert_eq!(failure.location, "rule.broken.actions[0]");
        assert!(failure.hint.contains("missing_channel"));
        assert!(!result.as_json().contains("UnknownCreatedChannelRef"));
    });
}

#[test]
fn golden_trace_reports_missing_private_overwrite() {
    block_on(async {
        let mut draft = support::golden_draft().await;
        let submit = draft
            .ruleset
            .rules
            .iter_mut()
            .find(|rule| rule.key == "submit_room")
            .unwrap();
        submit.actions.retain(|action| {
            !matches!(
                action,
                ActionSpec::UpsertOverwrite {
                    target: automation_state::OverwriteTargetSpec::Everyone,
                    ..
                }
            )
        });
        draft.draft_revision += 1;

        assert!(dispatch_tool(&mut draft, "validate_draft", "{}")
            .await
            .is_ok());
        let result = dispatch_tool(&mut draft, "simulate_draft", "{}").await;

        let failure = result.failure().unwrap();
        assert_eq!(failure.code, "GOLDEN_TRACE_PRIVATE_OVERWRITE_MISSING");
        assert_eq!(failure.location, "simulation.overwrites");
        assert_eq!(draft.simulated_revision, None);
    });
}

#[test]
fn failed_simulation_is_repaired_incrementally_on_the_same_draft() {
    block_on(async {
        let mut draft = support::golden_draft().await;
        let submit = draft
            .ruleset
            .rules
            .iter_mut()
            .find(|rule| rule.key == "submit_room")
            .unwrap();
        submit.actions.retain(|action| {
            !matches!(
                action,
                ActionSpec::UpsertOverwrite {
                    target: automation_state::OverwriteTargetSpec::Everyone,
                    ..
                }
            )
        });
        draft.draft_revision += 1;

        assert!(dispatch_tool(&mut draft, "validate_draft", "{}")
            .await
            .is_ok());
        assert_eq!(
            dispatch_tool(&mut draft, "simulate_draft", "{}")
                .await
                .failure()
                .unwrap()
                .code,
            "GOLDEN_TRACE_PRIVATE_OVERWRITE_MISSING"
        );
        let failed_revision = draft.draft_revision;

        let repair = dispatch_tool(
            &mut draft,
            "add_upsert_overwrite_action",
            &json!({
                "rule_key":"submit_room",
                "channel":{"kind":"created","name":"room_channel"},
                "target_kind":"everyone",
                "allow":[],
                "deny":["view_channel"]
            })
            .to_string(),
        )
        .await;

        assert!(repair.is_ok(), "{}", repair.as_json());
        assert_eq!(draft.draft_revision, failed_revision + 1);
        assert!(dispatch_tool(&mut draft, "validate_draft", "{}")
            .await
            .is_ok());
        assert!(dispatch_tool(&mut draft, "simulate_draft", "{}")
            .await
            .is_ok());
    });
}
