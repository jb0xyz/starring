use super::intent_interpretation::{
    EconomyRequirementV2, IntentRequestModeV2, PersistenceRequirementV2, TimerRequirementV2,
};
use super::intent_request_mode_grounding::grounded_request_controls;
use super::intent_runtime_grounding::{
    ground_runtime_requirements, requirement_action_occurrence_scans,
};

fn ground(human: &str) -> super::RuntimeRequirementsV2 {
    let controls = grounded_request_controls(human);
    ground_runtime_requirements(&controls.active_semantic_units.unwrap()).unwrap()
}

#[test]
fn runtime_grounding_recovers_each_explicit_infrastructure_axis() {
    let grounded = ground(
        "Build a persistent Discord game where every message earns XP, levels unlock an economy, timers advance quests, and an LLM decides rewards at event time. Quest timers must be durable, and the economy ledger must be persistent. Preserve state across restarts.",
    );

    assert_eq!(
        grounded.persistence,
        PersistenceRequirementV2::RestartPersistent
    );
    assert_eq!(grounded.timers, TimerRequirementV2::Durable);
    assert_eq!(grounded.economy, EconomyRequirementV2::PersistentLedger);
    assert!(grounded.event_time_llm);
}

#[test]
fn runtime_grounding_clears_inferred_recipe_infrastructure() {
    for human in [
        "Now build the managed private study-room automation and prepare its validated preview. Use English default copy and naming, community_hub as the existing discovery hub, and leave room closing disabled.",
        "Build a private study room with built-in roles, channels, panels, and timers.",
        "Create a static feedback modal and send an ephemeral response on submission.",
    ] {
        let grounded = ground(human);
        assert_eq!(grounded.persistence, PersistenceRequirementV2::None);
        assert_eq!(grounded.timers, TimerRequirementV2::None, "{human}");
        assert_eq!(grounded.economy, EconomyRequirementV2::None, "{human}");
        assert!(!grounded.event_time_llm);
    }
}

#[test]
fn runtime_grounding_ignores_quotes_examples_and_opt_outs() {
    for human in [
        "Build a static panel labelled 'durable timers and persistent economy'.",
        "Build a help panel. The phrase restart persistence is an example prompt.",
        "Build the room without durable timers and without restart persistence.",
        "Build the game, but the economy ledger does not need to be persistent.",
        "Build a static response without an LLM at event time.",
        "Build a game, but do not use persistent state, durable timers, a persistent economy, or an LLM at event time.",
        "Build the game, but do not add durable timers.",
        "Build a restart-aware flow, but do not preserve state across restarts.",
        "영속 타이머 없이 게임을 만들고 경제는 영속적일 필요 없게 해줘.",
        "게임은 만들되 영속 타이머는 쓰지 마.",
        "Build a game where state must not survive restarts.",
        "Build a game. State does not survive restarts.",
        "Persistent state must not be used across restarts.",
        "At event time, the LLM must not decide rewards.",
        "이벤트 시점에 AI가 보상을 결정하지 않게 해줘.",
        "Durable timers must not be used.",
        "The persistent economy must not be used.",
        "영속 경제를 쓰지 마.",
        "영속 상태를 쓰지 마.",
        "Build a game and never use durable timers.",
        "게임을 만들되 영속 타이머는 사용하지 마.",
        "영속 타이머와 영속 경제를 사용하지 마.",
    ] {
        let grounded = ground(human);
        assert_eq!(grounded.persistence, PersistenceRequirementV2::None);
        assert_eq!(grounded.timers, TimerRequirementV2::None);
        assert_eq!(grounded.economy, EconomyRequirementV2::None);
        assert!(!grounded.event_time_llm);
    }
}

#[test]
fn runtime_grounding_keeps_compound_polite_build_requirements_authoritative() {
    let grounded = ground("Can you build a game and use durable timers?");
    assert_eq!(grounded.timers, TimerRequirementV2::Durable);

    let grounded = ground("게임 자동화를 만들어 줄래, 그리고 영속 타이머를 사용해 줄래?");
    assert_eq!(grounded.timers, TimerRequirementV2::Durable);

    let grounded = ground("아마존 게임에 영속 타이머를 추가해줘.");
    assert_eq!(grounded.timers, TimerRequirementV2::Durable);
}

#[test]
fn runtime_grounding_limits_question_authority_to_direct_build_continuations() {
    let grounded = ground("Are durable timers necessary, and can you build a static game?");
    assert_eq!(grounded.timers, TimerRequirementV2::None);

    let grounded = ground(
        "At runtime use static rules, and should we use durable timers, and let an LLM decide setup copy.",
    );
    assert_eq!(grounded.timers, TimerRequirementV2::None);
    assert!(!grounded.event_time_llm);

    let controls = grounded_request_controls(
        "Should we discuss naming, and can you build a game and use durable timers?",
    );
    assert_eq!(controls.mode, Some(IntentRequestModeV2::Build));
    let grounded = ground_runtime_requirements(&controls.active_semantic_units.unwrap()).unwrap();
    assert_eq!(grounded.timers, TimerRequirementV2::Durable);

    let controls =
        grounded_request_controls("If durable timers are available, build a static game.");
    assert_eq!(controls.mode, None);
    let grounded = ground_runtime_requirements(&controls.active_semantic_units.unwrap()).unwrap();
    assert_eq!(grounded.timers, TimerRequirementV2::None);

    let controls =
        grounded_request_controls("Consider durable timers, and now build a static game.");
    assert_eq!(controls.mode, Some(IntentRequestModeV2::Build));
    let grounded = ground_runtime_requirements(&controls.active_semantic_units.unwrap()).unwrap();
    assert_eq!(grounded.timers, TimerRequirementV2::None);

    let controls =
        grounded_request_controls("If durable timers are available and build a static game.");
    assert_eq!(controls.mode, None);

    let grounded = ground(
        "Can you build a game, but are durable timers necessary and use a persistent economy?",
    );
    assert_eq!(grounded.timers, TimerRequirementV2::None);
    assert_eq!(grounded.economy, EconomyRequirementV2::None);
}

#[test]
fn runtime_grounding_rejects_cross_subject_and_undecided_cooccurrence() {
    for human in [
        "Build a reward flow where moderation logs must be persistent.",
        "Build a timer for a persistent moderator role.",
        "At runtime, choose whether to enable AI. Build a static panel.",
        "Maybe use a persistent economy later, but build a static game now.",
    ] {
        let grounded = ground(human);
        assert_eq!(grounded.persistence, PersistenceRequirementV2::None);
        assert_eq!(grounded.timers, TimerRequirementV2::None);
        assert_eq!(grounded.economy, EconomyRequirementV2::None);
        assert!(!grounded.event_time_llm);
    }
}

#[test]
fn runtime_grounding_keeps_requirements_when_negation_targets_another_subject() {
    let grounded = ground("Build durable timers without persistent moderation logs.");
    assert_eq!(grounded.timers, TimerRequirementV2::Durable);
    assert_eq!(grounded.persistence, PersistenceRequirementV2::None);

    let grounded = ground("Preserve state across restarts without retaining audit logs.");
    assert_eq!(
        grounded.persistence,
        PersistenceRequirementV2::RestartPersistent
    );

    let grounded = ground("Use a persistent economy without storing user profiles.");
    assert_eq!(grounded.economy, EconomyRequirementV2::PersistentLedger);

    let grounded = ground("At runtime, call an LLM without logging prompts.");
    assert!(grounded.event_time_llm);

    let grounded = ground(
        "Use a persistent economy without storing user profiles, and at runtime call an LLM.",
    );
    assert_eq!(grounded.economy, EconomyRequirementV2::PersistentLedger);
    assert!(grounded.event_time_llm);
}

#[test]
fn runtime_grounding_keeps_runtime_axes_owned_by_their_subjects() {
    for human in [
        "Timers survive restarts.",
        "Quest timers must survive restarts.",
    ] {
        let grounded = ground(human);
        assert_eq!(grounded.persistence, PersistenceRequirementV2::None);
        assert_eq!(grounded.timers, TimerRequirementV2::Durable);
    }

    for human in ["XP survives restarts.", "Keep XP across restarts."] {
        let grounded = ground(human);
        assert_eq!(grounded.persistence, PersistenceRequirementV2::None);
        assert_eq!(grounded.economy, EconomyRequirementV2::PersistentLedger);
    }

    let grounded = ground("타이머는 재시작 후에도 유지해야 해.");
    assert_eq!(grounded.persistence, PersistenceRequirementV2::None);
    assert_eq!(grounded.timers, TimerRequirementV2::Durable);

    let grounded = ground("Keep state across restarts.");
    assert_eq!(
        grounded.persistence,
        PersistenceRequirementV2::RestartPersistent
    );
}

#[test]
fn runtime_grounding_does_not_merge_alternatives_or_reverse_event_context() {
    for human in [
        "Build a game using persistent state or durable timers.",
        "Use durable timers or a persistent economy.",
        "Build a game that at runtime uses static rules or calls an LLM.",
        "Use durable timers or persistent timers.",
        "Use durable timers or persistent timers and a persistent economy.",
        "Use durable timers and a persistent economy or persistent timers and a persistent economy.",
        "Can you build a static game, or should we use durable timers?",
        "At runtime, use static rules or call an LLM.",
        "At runtime, call an LLM or use static rules.",
        "Use durable timers or static scheduling.",
        "Use durable timers during quests or use static scheduling.",
        "Use durable timers for quests or choose static scheduling.",
        "Use durable timers for quests or reminders.",
        "Use durable timers during quests or schedule statically.",
        "Use durable timers during quests or disable them.",
        "게임에는 영속 타이머 또는 영속 경제 중 하나만 사용해줘.",
        "영속 타이머 또는 정적 스케줄링을 사용해줘.",
        "영속 타이머를 퀘스트 또는 알림에 사용해줘.",
        "영속 타이머를 퀘스트에 사용하거나 정적 스케줄링을 사용해줘.",
        "영속 타이머나 영속 경제를 사용해줘.",
    ] {
        let controls = grounded_request_controls(human);
        assert!(
            ground_runtime_requirements(&controls.active_semantic_units.unwrap()).is_err(),
            "{human}"
        );
    }

    assert!(
        !ground("Use an LLM to generate setup copy, and at runtime use static rules.")
            .event_time_llm
    );
}

#[test]
fn runtime_grounding_keeps_non_authoritative_scope_across_connected_units() {
    for human in [
        "Maybe use durable timers or a persistent economy, but build a static game now.",
        "Consider durable timers and a persistent economy, but build static.",
        "We could use durable timers and a persistent economy later. Build static.",
        "Compare durable timers with a persistent economy, then build a static game.",
        "Explain what durable timers are, then build a static panel.",
        "영속 타이머와 영속 경제를 비교하고 정적 게임을 만들어줘.",
    ] {
        let grounded = ground(human);
        assert_eq!(grounded.timers, TimerRequirementV2::None, "{human}");
        assert_eq!(grounded.economy, EconomyRequirementV2::None, "{human}");
    }
}

#[test]
fn runtime_grounding_separates_setup_negation_from_event_time_requirements() {
    for human in [
        "Generate onboarding copy without an LLM. At event time, an LLM decides rewards.",
        "Do not call an LLM during setup. At event time, the LLM decides rewards.",
    ] {
        assert!(ground(human).event_time_llm);
    }
}

#[test]
fn runtime_grounding_recognizes_event_actions_without_global_meta_suppression() {
    for human in [
        "At runtime, calculate a score and let an LLM decide rewards.",
        "At runtime, handle messages, and let an LLM decide rewards.",
        "At runtime, an LLM decides rewards using policy generated during setup.",
        "At runtime, an LLM evaluates a policy prepared at setup time.",
        "At runtime, use static rules, and an LLM evaluates a policy prepared at setup time.",
        "At runtime, an LLM evaluates the setup copy.",
        "At runtime, an LLM decides whether a reward is valid.",
        "At runtime, compare scores and let AI decide a winner.",
        "이벤트 시점에 AI가 보상 여부를 결정해줘.",
        "이벤트 시점에 AI가 사용자 선택을 고려해 보상을 결정해줘.",
    ] {
        assert!(ground(human).event_time_llm, "{human}");
    }
}

#[test]
fn runtime_grounding_rejects_optional_and_non_runtime_lexical_lookalikes() {
    for human in [
        "Build static rules; durable timers are available but not needed.",
        "Optionally use durable timers.",
        "You may use a persistent economy.",
        "When available, use durable timers.",
        "영속 타이머는 선택 사항이야. 정적 게임을 만들어줘.",
        "가능하면 영속 타이머를 사용해도 돼.",
        "Create a help panel explaining why durable timers are unnecessary.",
    ] {
        let grounded = ground(human);
        assert_eq!(grounded.timers, TimerRequirementV2::None, "{human}");
        assert_eq!(grounded.economy, EconomyRequirementV2::None, "{human}");
    }

    let grounded = ground(
        "Create a persistent reward message that remains pinned, but do not store any reward balances.",
    );
    assert_eq!(grounded.economy, EconomyRequirementV2::None);

    assert!(!ground("At runtime, use an LLM role as a permission marker.").event_time_llm);

    let grounded = ground("Use durable timers; approval is not required.");
    assert_eq!(grounded.timers, TimerRequirementV2::Durable);
}

#[test]
fn runtime_grounding_recognizes_extended_negative_predicates() {
    for human in [
        "Build a game without using durable timers.",
        "Build a game and not use durable timers.",
    ] {
        assert_eq!(ground(human).timers, TimerRequirementV2::None, "{human}");
    }

    for human in [
        "Build without using persistent state.",
        "We cannot use persistent state. Build a static game.",
    ] {
        assert_eq!(
            ground(human).persistence,
            PersistenceRequirementV2::None,
            "{human}"
        );
    }

    for human in [
        "At runtime, do not let an LLM decide rewards.",
        "At runtime, don't ask an LLM to decide rewards.",
    ] {
        assert!(!ground(human).event_time_llm, "{human}");
    }
}

#[test]
fn runtime_grounding_honors_forbidden_and_survival_negations() {
    for human in [
        "Persistent state is forbidden in this game.",
        "Build a game where durable timers are forbidden.",
        "At runtime, the LLM is forbidden to decide rewards.",
        "At runtime, never let an LLM decide rewards.",
        "영속 타이머를 쓰면 안 돼.",
        "이벤트 시점에 AI가 보상을 결정하면 안 돼.",
        "Timers must not survive restarts.",
        "XP must not persist across restarts.",
        "The economy must not survive restarts.",
    ] {
        let grounded = ground(human);
        assert_eq!(
            grounded.persistence,
            PersistenceRequirementV2::None,
            "{human}"
        );
        assert_eq!(grounded.timers, TimerRequirementV2::None, "{human}");
        assert_eq!(grounded.economy, EconomyRequirementV2::None, "{human}");
        assert!(!grounded.event_time_llm, "{human}");
    }

    for human in [
        "Use durable timers. Timers must not survive restarts.",
        "Use a persistent economy. XP must not persist across restarts.",
    ] {
        let controls = grounded_request_controls(human);
        assert!(
            ground_runtime_requirements(&controls.active_semantic_units.unwrap()).is_err(),
            "{human}"
        );
    }
}

#[test]
fn runtime_grounding_excludes_embedded_ui_questions() {
    for human in [
        "Can you build a panel that asks whether durable timers are needed?",
        "Can you build a panel that asks whether a persistent economy is needed?",
        "Can you build a panel that asks whether an LLM should decide rewards at event time?",
    ] {
        let grounded = ground(human);
        assert_eq!(grounded.timers, TimerRequirementV2::None, "{human}");
        assert_eq!(grounded.economy, EconomyRequirementV2::None, "{human}");
        assert!(!grounded.event_time_llm, "{human}");
    }
}

#[test]
fn runtime_grounding_stops_inherited_event_scope_at_setup_boundaries() {
    assert!(
        !ground("At runtime, use static rules, and during setup use an LLM to generate copy.")
            .event_time_llm
    );
    assert!(ground("At runtime, use static rules, and let an LLM decide rewards.").event_time_llm);
    assert!(
        ground("At runtime, call an LLM for rewards, and during setup do not call an LLM.")
            .event_time_llm
    );
}

#[test]
fn runtime_grounding_recovers_authority_after_explicit_scope_reset() {
    let grounded =
        ground("Maybe use durable timers, and then definitely use a persistent economy.");
    assert_eq!(grounded.timers, TimerRequirementV2::None);
    assert_eq!(grounded.economy, EconomyRequirementV2::PersistentLedger);

    let grounded = ground("영속 타이머를 고려하고, 이제 영속 경제를 사용해줘.");
    assert_eq!(grounded.timers, TimerRequirementV2::None);
    assert_eq!(grounded.economy, EconomyRequirementV2::PersistentLedger);

    let controls =
        grounded_request_controls("Maybe use durable timers, and please, now build a static game.");
    assert_eq!(controls.mode, Some(IntentRequestModeV2::Build));
    let grounded = ground_runtime_requirements(&controls.active_semantic_units.unwrap()).unwrap();
    assert_eq!(grounded.timers, TimerRequirementV2::None);

    for human in [
        "Maybe use durable timers, and then definitely consider a persistent economy.",
        "Maybe use durable timers, and now maybe use a persistent economy.",
        "영속 타이머를 고려하고, 이제 영속 경제를 고려해줘.",
    ] {
        let grounded = ground(human);
        assert_eq!(grounded.timers, TimerRequirementV2::None, "{human}");
        assert_eq!(grounded.economy, EconomyRequirementV2::None, "{human}");
    }
}

#[test]
fn runtime_grounding_bounds_repeated_marker_work() {
    let repeated = "AI ".repeat(10_000);
    let grounded = ground(&format!(
        "Build a game. At runtime, {repeated}AI decides rewards."
    ));
    assert!(grounded.event_time_llm);

    let repeated = "AI decides ".repeat(10_000);
    let grounded = ground(&format!("Build a game. At runtime, {repeated}"));
    assert!(grounded.event_time_llm);
}

#[test]
fn runtime_grounding_ends_distributed_negation_at_an_explicit_positive_predicate() {
    let grounded = ground("Build a game. Do not use persistent state and preserve durable timers.");
    assert_eq!(grounded.persistence, PersistenceRequirementV2::None);
    assert_eq!(grounded.timers, TimerRequirementV2::Durable);
}

#[test]
fn runtime_grounding_supports_korean_explicit_requirements() {
    let grounded = ground(
        "재시작 후에도 상태를 유지하고 영속 타이머로 퀘스트를 진행하며 영속 경험치 저장소를 사용해줘. 이벤트 시점에는 AI가 보상을 결정하게 해줘.",
    );

    assert_eq!(
        grounded.persistence,
        PersistenceRequirementV2::RestartPersistent
    );
    assert_eq!(grounded.timers, TimerRequirementV2::Durable);
    assert_eq!(grounded.economy, EconomyRequirementV2::PersistentLedger);
    assert!(grounded.event_time_llm);
}

#[test]
fn runtime_grounding_rejects_unmatched_quotes_before_classification() {
    assert!(
        grounded_request_controls("Build a game with 'durable timers")
            .active_semantic_units
            .is_none()
    );
}

#[test]
fn runtime_grounding_excludes_copied_and_hypothetical_units() {
    let grounded = ground(
        "Example prompt: Build a game with durable timers. Preserve state across restarts. End of example. Build a static feedback panel.",
    );
    assert_eq!(grounded.persistence, PersistenceRequirementV2::None);
    assert_eq!(grounded.timers, TimerRequirementV2::None);

    let grounded =
        ground("What if an LLM decided rewards at event time? Build a static feedback panel.");
    assert!(!grounded.event_time_llm);

    for human in [
        "If durable timers are available, build a static game.",
        "We could use durable timers later. Build a static panel.",
    ] {
        assert_eq!(ground(human).timers, TimerRequirementV2::None);
    }

    let grounded = ground("At runtime use static rules, and use an LLM only during setup.");
    assert!(!grounded.event_time_llm);

    let grounded = ground("At runtime, use the LLM-generated setup copy in a static response.");
    assert!(!grounded.event_time_llm);
}

#[test]
fn runtime_grounding_recovers_authority_after_a_copy_terminator() {
    let grounded = ground(
        "Example prompt: Build a static panel. End of example. Build a game whose state must survive restarts.",
    );
    assert_eq!(
        grounded.persistence,
        PersistenceRequirementV2::RestartPersistent
    );
}

#[test]
fn runtime_grounding_does_not_truncate_domain_words_as_ui_copy() {
    let grounded = ground("Use context to preserve state across restarts.");
    assert_eq!(
        grounded.persistence,
        PersistenceRequirementV2::RestartPersistent
    );

    let grounded = ground("Build a label management workflow with durable timers.");
    assert_eq!(grounded.timers, TimerRequirementV2::Durable);

    let grounded = ground("Create a button named Durable Timers.");
    assert_eq!(grounded.timers, TimerRequirementV2::None);
}

#[test]
fn runtime_grounding_fails_closed_on_conflicting_requirements() {
    for human in [
        "Build a game with durable timers, but timers do not need to be durable.",
        "Can you build a game with durable timers, but do not use durable timers?",
    ] {
        let controls = grounded_request_controls(human);
        assert!(
            ground_runtime_requirements(&controls.active_semantic_units.unwrap()).is_err(),
            "{human}"
        );
    }
}

#[test]
fn runtime_grounding_handles_closed_negation_grammar() {
    for human in [
        "State must never survive restarts. Build a static game.",
        "Persistent state is prohibited. Build a static game.",
        "Build a game where durable timers are not allowed.",
        "A persistent economy is disallowed. Build a static game.",
        "At runtime, the LLM is not allowed to decide rewards.",
        "At runtime, do not allow the LLM to decide rewards.",
        "Use neither durable timers nor a persistent economy.",
        "Avoid durable timers and a persistent economy.",
        "영속 경제는 금지야. 정적 게임을 만들어줘.",
        "Build a game, but don’t use durable timers.",
        "Build a game and disable durable timers.",
        "Build a game and remove persistent economy support.",
        "We don't need durable timers.",
        "A persistent economy isn’t required.",
        "영속 타이머가 필요하지 않아.",
    ] {
        let grounded = ground(human);
        assert_eq!(
            grounded.persistence,
            PersistenceRequirementV2::None,
            "{human}"
        );
        assert_eq!(grounded.timers, TimerRequirementV2::None, "{human}");
        assert_eq!(grounded.economy, EconomyRequirementV2::None, "{human}");
        assert!(!grounded.event_time_llm, "{human}");
    }

    let grounded = ground("Durable timers are prohibited, but use a persistent economy.");
    assert_eq!(grounded.timers, TimerRequirementV2::None);
    assert_eq!(grounded.economy, EconomyRequirementV2::PersistentLedger);

    let grounded = ground("Use a persistent economy, but state must never survive restarts.");
    assert_eq!(grounded.persistence, PersistenceRequirementV2::None);
    assert_eq!(grounded.economy, EconomyRequirementV2::PersistentLedger);

    let grounded =
        ground("Use durable timers, but audit records must not persist across restarts.");
    assert_eq!(grounded.persistence, PersistenceRequirementV2::None);
    assert_eq!(grounded.timers, TimerRequirementV2::Durable);

    let grounded = ground("At runtime, call an LLM, but audit logs must never persist.");
    assert_eq!(grounded.persistence, PersistenceRequirementV2::None);
    assert!(grounded.event_time_llm);
}

#[test]
fn runtime_grounding_owns_ui_copy_without_erasing_active_clauses() {
    for human in [
        "Build a panel asking whether durable timers are needed.",
        "Build a panel prompting users whether a persistent economy is needed.",
        "Build a modal whose text says an LLM should decide rewards at event time.",
        "Build a help panel explaining how persistent state works across restarts.",
        "Build a help panel describing durable timers.",
        "Build a button whose caption is durable timers.",
        "Build a help panel about durable timers.",
        "Build a comparison panel about persistent economy options.",
        "영속 타이머가 필요한지 묻는 패널을 만들어줘.",
        "영속 경제가 필요한지 질문하는 모달을 만들어줘.",
        "이벤트 시점에 AI가 보상을 결정하는지 묻는 패널을 만들어줘.",
    ] {
        let grounded = ground(human);
        assert_eq!(
            grounded.persistence,
            PersistenceRequirementV2::None,
            "{human}"
        );
        assert_eq!(grounded.timers, TimerRequirementV2::None, "{human}");
        assert_eq!(grounded.economy, EconomyRequirementV2::None, "{human}");
        assert!(!grounded.event_time_llm, "{human}");
    }

    let grounded = ground("Build a game named 'Quest' with durable timers.");
    assert_eq!(grounded.timers, TimerRequirementV2::Durable);

    let grounded = ground("Build a game called 'Quest' whose state survives restarts.");
    assert_eq!(
        grounded.persistence,
        PersistenceRequirementV2::RestartPersistent
    );

    assert!(
        ground("Build a game that at runtime asks whether a user qualifies by calling an LLM.")
            .event_time_llm
    );
    assert!(
        ground("Build a game where the LLM is called at runtime to decide rewards.").event_time_llm
    );
}

#[test]
fn runtime_grounding_separates_setup_scope_and_passive_runtime_execution() {
    for human in [
        "At runtime, use static rules, and while setting up use an LLM to generate copy.",
        "At runtime, use static rules, and for setup use an LLM to generate copy.",
        "At runtime, use static rules, and before launch use an LLM to generate copy.",
        "At runtime, use static rules, and then use an LLM to generate setup copy.",
        "At runtime, use static rules, and once during initialization use an LLM to generate copy.",
        "At runtime, use static rules, and use an LLM during setup to generate copy.",
        "실행 시점에는 정적 규칙을 사용하고, 설정 단계에서는 AI가 문구를 생성해줘.",
    ] {
        assert!(!ground(human).event_time_llm, "{human}");
    }

    for human in [
        "At runtime, use static rules, and then use an LLM to generate the response.",
        "At runtime, use an LLM to evaluate copy prepared during setup.",
        "At runtime, rewards are decided by an LLM.",
    ] {
        assert!(ground(human).event_time_llm, "{human}");
    }
}

#[test]
fn runtime_grounding_fails_closed_on_extended_alternative_frames() {
    for human in [
        "Choose between durable timers and a persistent economy.",
        "Use one of durable timers and a persistent economy.",
        "Use durable timers and/or a persistent economy.",
        "Use durable timers, otherwise use static scheduling.",
        "영속 타이머와 영속 경제 중 하나를 사용해줘.",
        "영속 타이머든 영속 경제든 하나를 사용해줘.",
    ] {
        let controls = grounded_request_controls(human);
        assert!(
            ground_runtime_requirements(&controls.active_semantic_units.unwrap()).is_err(),
            "{human}"
        );
    }

    assert_eq!(
        ground("Use durable timers rather than static scheduling.").timers,
        TimerRequirementV2::Durable
    );

    for human in [
        "영속 타이머 하나 추가해줘.",
        "영속 타이머를 누구나 사용할 수 있게 해줘.",
    ] {
        assert_eq!(ground(human).timers, TimerRequirementV2::Durable, "{human}");
    }
}

#[test]
fn runtime_grounding_resets_hypothetical_and_negative_scope_on_explicit_requirements() {
    for human in [
        "Maybe use durable timers, and then use a persistent economy.",
        "Consider durable timers, then use a persistent economy.",
        "영속 타이머를 쓸 수도 있지만, 이제 영속 경제를 사용해줘.",
        "영속 타이머를 고려하고, 반드시 영속 경제를 사용해줘.",
        "Maybe use durable timers. Use a persistent economy.",
        "If durable timers are available, then definitely use a persistent economy.",
        "Should we use durable timers? Now use a persistent economy.",
    ] {
        let grounded = ground(human);
        assert_eq!(grounded.timers, TimerRequirementV2::None, "{human}");
        assert_eq!(
            grounded.economy,
            EconomyRequirementV2::PersistentLedger,
            "{human}"
        );
    }

    for human in [
        "Durable timers might be useful, but build a static game.",
        "Persistent economy could be useful, but build a static game.",
        "Maybe use durable timers and a persistent economy.",
    ] {
        let grounded = ground(human);
        assert_eq!(grounded.timers, TimerRequirementV2::None, "{human}");
        assert_eq!(grounded.economy, EconomyRequirementV2::None, "{human}");
    }

    for human in [
        "Do not use persistent state, and definitely use durable timers.",
        "Do not use persistent state, and timers must be durable.",
    ] {
        let grounded = ground(human);
        assert_eq!(
            grounded.persistence,
            PersistenceRequirementV2::None,
            "{human}"
        );
        assert_eq!(grounded.timers, TimerRequirementV2::Durable, "{human}");
    }
}

#[test]
fn runtime_grounding_covers_adjacent_negation_and_axis_ownership() {
    for human in [
        "State should never survive restarts.",
        "State can never survive restarts.",
        "Do not keep state across restarts.",
        "Never keep state across restarts.",
        "Durable timers must never be used.",
        "A persistent economy must never be used.",
        "At runtime, the LLM cannot decide rewards.",
        "At runtime, the LLM may not decide rewards.",
        "At runtime, don't permit the LLM to decide rewards.",
        "상태를 재시작 후에도 유지하면 안 돼.",
        "이벤트 시점에 AI가 보상을 결정해서는 안 돼.",
        "Build a static game; no need for durable timers.",
    ] {
        let grounded = ground(human);
        assert_eq!(
            grounded.persistence,
            PersistenceRequirementV2::None,
            "{human}"
        );
        assert_eq!(grounded.timers, TimerRequirementV2::None, "{human}");
        assert_eq!(grounded.economy, EconomyRequirementV2::None, "{human}");
        assert!(!grounded.event_time_llm, "{human}");
    }

    let grounded = ground("Use durable timers, but never keep audit records across restarts.");
    assert_eq!(grounded.persistence, PersistenceRequirementV2::None);
    assert_eq!(grounded.timers, TimerRequirementV2::Durable);

    let grounded = ground("Use durable timers, and remove the timer panel.");
    assert_eq!(grounded.timers, TimerRequirementV2::Durable);

    let grounded = ground("Use a persistent economy, and remove the reward role.");
    assert_eq!(grounded.economy, EconomyRequirementV2::PersistentLedger);
}

#[test]
fn runtime_grounding_covers_adjacent_ui_and_setup_ownership() {
    for human in [
        "Build a panel asking users whether durable timers are required.",
        "Build a modal that prompts the user whether a persistent economy is required.",
        "Build a panel posing whether durable timers are required.",
        "Build a help panel explaining when state should survive restarts.",
        "영속 타이머가 필요한지 안내하는 패널을 만들어줘.",
        "영속 경제가 필요한지 확인하는 모달을 만들어줘.",
        "이벤트 시점에 AI가 보상을 결정하는지 확인하는 패널을 만들어줘.",
    ] {
        let grounded = ground(human);
        assert_eq!(
            grounded.persistence,
            PersistenceRequirementV2::None,
            "{human}"
        );
        assert_eq!(grounded.timers, TimerRequirementV2::None, "{human}");
        assert_eq!(grounded.economy, EconomyRequirementV2::None, "{human}");
        assert!(!grounded.event_time_llm, "{human}");
    }

    for human in [
        "At runtime, use static rules, and during initialization use an LLM to generate copy.",
        "At runtime, use static rules, and at initialization time use an LLM to generate copy.",
        "At runtime, use static rules, and in the setup phase use an LLM to generate copy.",
        "At runtime, use static rules, and use an LLM to generate initialization copy.",
        "실행 시점에는 정적 규칙을 사용하고, 초기 설정 때 AI가 문구를 생성해줘.",
    ] {
        assert!(!ground(human).event_time_llm, "{human}");
    }

    assert!(
        ground("Build a game that at runtime asks users whether they qualify by calling an LLM.")
            .event_time_llm
    );
    assert_eq!(
        ground("Build a control panel for a game called 'Quest' whose state survives restarts.")
            .persistence,
        PersistenceRequirementV2::RestartPersistent
    );
    assert!(!ground("At runtime, use an LLM role called moderator.").event_time_llm);
    assert!(!ground("Build a panel that at runtime says an LLM decides rewards.").event_time_llm);
}

#[test]
fn runtime_grounding_covers_adjacent_alternative_grammar() {
    for human in [
        "Pick between durable timers and a persistent economy.",
        "Select one from durable timers and a persistent economy.",
        "Use durable timers xor a persistent economy.",
        "Choose durable timers versus a persistent economy.",
        "영속 타이머와 영속 경제 중 택일해줘.",
        "영속 타이머 또는 영속 경제를 골라줘.",
        "영속 타이머나 정적 스케줄링을 사용해줘.",
    ] {
        let controls = grounded_request_controls(human);
        assert!(
            ground_runtime_requirements(&controls.active_semantic_units.unwrap()).is_err(),
            "{human}"
        );
    }

    for human in [
        "Use durable timers in one of the rooms.",
        "Choose between rooms, then use durable timers.",
    ] {
        assert_eq!(ground(human).timers, TimerRequirementV2::Durable, "{human}");
    }
}

#[test]
fn runtime_grounding_covers_adjacent_hypothetical_and_action_ownership() {
    for human in [
        "Perhaps use durable timers, and then use a persistent economy.",
        "Potentially use durable timers, then use a persistent economy.",
        "영속 타이머가 필요할 수도 있지만, 이제 영속 경제를 사용해줘.",
        "Maybe require durable timers, but definitely use a persistent economy.",
        "We could require durable timers, then use a persistent economy.",
    ] {
        let grounded = ground(human);
        assert_eq!(grounded.timers, TimerRequirementV2::None, "{human}");
        assert_eq!(
            grounded.economy,
            EconomyRequirementV2::PersistentLedger,
            "{human}"
        );
    }

    for human in [
        "Durable timers might be required, but build a static game.",
        "A persistent economy may be required, but build a static game.",
        "Durable timers can be used, but build a static game.",
    ] {
        let grounded = ground(human);
        assert_eq!(grounded.timers, TimerRequirementV2::None, "{human}");
        assert_eq!(grounded.economy, EconomyRequirementV2::None, "{human}");
    }

    let grounded = ground("Use a persistent economy to document durable timers.");
    assert_eq!(grounded.economy, EconomyRequirementV2::PersistentLedger);
    assert_eq!(grounded.timers, TimerRequirementV2::None);
}

#[test]
fn runtime_grounding_does_not_invert_extended_negative_requirements() {
    for human in [
        "Persistent state must not be included.",
        "A persistent economy must not be included.",
        "State shouldn't survive restarts.",
        "State can't survive restarts.",
    ] {
        let grounded = ground(human);
        assert_eq!(
            grounded.persistence,
            PersistenceRequirementV2::None,
            "{human}"
        );
        assert_eq!(grounded.timers, TimerRequirementV2::None, "{human}");
        assert_eq!(grounded.economy, EconomyRequirementV2::None, "{human}");
        assert!(!grounded.event_time_llm, "{human}");
    }
}

#[test]
fn runtime_grounding_fails_closed_on_intra_unit_conflicts_and_conditionals() {
    for human in [
        "Use durable timers without durable timers.",
        "Use durable timers unless static scheduling is selected.",
        "영속 타이머를 사용해줘. 하지만 영속 타이머는 안 써줘.",
    ] {
        let controls = grounded_request_controls(human);
        assert!(
            ground_runtime_requirements(&controls.active_semantic_units.unwrap()).is_err(),
            "{human}"
        );
    }

    assert_eq!(
        ground("Use durable timers without documenting durable timers.").timers,
        TimerRequirementV2::Durable
    );
}

#[test]
fn runtime_grounding_owns_postposed_setup_and_llm_subjects() {
    for human in [
        "At runtime, use static rules, and use an LLM to generate copy during setup.",
        "At runtime, use static rules, and use an LLM to generate copy at setup time.",
        "At runtime, record which AI role a user chooses.",
        "At runtime, rewards chosen by users are sent to an LLM channel.",
    ] {
        assert!(!ground(human).event_time_llm, "{human}");
    }

    assert!(ground("At runtime, call an LLM, and remove the AI role.").event_time_llm);
}

#[test]
fn runtime_grounding_excludes_optional_and_documentation_ownership() {
    for human in [
        "Build a game that may use durable timers.",
        "Build a game that might need a persistent economy.",
        "Create a workflow to document how to use durable timers.",
    ] {
        let grounded = ground(human);
        assert_eq!(grounded.timers, TimerRequirementV2::None, "{human}");
        assert_eq!(grounded.economy, EconomyRequirementV2::None, "{human}");
    }
}

#[test]
fn runtime_grounding_handles_korean_contrast_and_persistence_predicates() {
    let grounded = ground("영속 타이머 말고 영속 경제를 사용해줘.");
    assert_eq!(grounded.timers, TimerRequirementV2::None);
    assert_eq!(grounded.economy, EconomyRequirementV2::PersistentLedger);

    for human in [
        "영속 상태를 유지해줘.",
        "게임 상태를 재시작해도 잃지 않게 해줘.",
    ] {
        assert_eq!(
            ground(human).persistence,
            PersistenceRequirementV2::RestartPersistent,
            "{human}"
        );
    }
}

#[test]
fn runtime_grounding_action_ownership_builds_fixed_term_indexes() {
    let small = ["use a static rule"; 128].join(" and ");
    let large = ["use a static rule"; 256].join(" and ");
    let small_scans = requirement_action_occurrence_scans(&small);
    let large_scans = requirement_action_occurrence_scans(&large);
    assert_eq!(small_scans, large_scans);
    assert!(small_scans > 0);
}
