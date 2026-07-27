const MIGRATION: &str =
    include_str!("../../../migrations/202607240010_persist_runtime_slot_writer_fence.sql");
const FIRST_APPLY_MIGRATION: &str =
    include_str!("../../../migrations/202607240009_add_product_drain_first_apply_core.sql");
const CONTRACT_SOURCE: &str = include_str!("../src/contract.rs");
const DATABASE_SOURCE: &str = include_str!("../src/database.rs");
const SECURITY_SUPPORT_SOURCE: &str = include_str!("postgres_security/support.rs");
const EXACT_TARGET_DATABASE_SOURCE: &str =
    include_str!("../../automation-runtime-convergence-postgres/src/hydration/database.rs");
const SERVING_DATABASE_SOURCE: &str =
    include_str!("../../automation-runtime-serving-postgres/src/database.rs");

const FENCE_TABLE: &str = "public.runtime_slot_writer_fences_v2";
const CREATE_IDENTITY: &str = "starring_runtime_private_v2.\
starring_runtime_slot_writer_fence_create_v2(text,text)";
const LOCK_IDENTITY: &str = "starring_runtime_private_v2.\
starring_runtime_slot_writer_fence_lock_v2(text,text)";
const BEGIN_UNSAFE_IDENTITY: &str = "starring_runtime_private_v2.\
starring_runtime_slot_writer_fence_begin_unsafe_v2(text,text,bigint)";
const MARK_DRAIN_IDENTITY: &str = "starring_runtime_private_v2.\
starring_runtime_slot_writer_fence_mark_drain_v2(\
text,text,bigint,text,text,text,text,text,bigint)";
const INSTALLATION_TRIGGER_IDENTITY: &str = "starring_runtime_private_v2.\
starring_runtime_slot_writer_fence_installation_insert_v2()";
const EXACT_TARGET_READINESS_DIGEST: &str =
    "e4bae4b38acc529accd4401af853eb7e96d2a34ad8fb1224b9965166ff40c229";
const SERVING_READINESS_DIGEST: &str =
    "1c0c79c6fbf528f28fb56e91a54b78cd1fe17c70d2bc3e8d7e3dc515d8a7f8f7";
const MIGRATION_READINESS_DIGEST: &str =
    "48a10f783603fe02879f2a1cddbecbb39541ac0ca154c77f7b1e0eef8d9f6834";
const CURRENT_READINESS_DIGEST: &str =
    "a5191ef59e5365476860af1150a176049ef00c5b0d6c3f7cfe40e0b5be9d738a";

fn function_section(marker: &str) -> &'static str {
    MIGRATION
        .split(marker)
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap()
}

fn function_body(marker: &str) -> &'static str {
    function_section(marker)
        .split("AS $function$")
        .nth(1)
        .unwrap()
}

fn dollar_block(tag: &str) -> &'static str {
    MIGRATION
        .split(&format!("DO ${tag}$"))
        .nth(1)
        .unwrap()
        .split(&format!("${tag}$;"))
        .next()
        .unwrap()
}

fn assert_invoker_helper(section: &str, strict: bool) {
    assert!(section.contains("LANGUAGE plpgsql"));
    assert!(section.contains("VOLATILE"));
    assert!(section.contains("PARALLEL UNSAFE"));
    assert!(section.contains("SECURITY INVOKER"));
    assert!(section.contains("SET search_path = pg_catalog"));
    assert_eq!(section.contains("\nSTRICT\n"), strict);
    assert!(!section.contains("SECURITY DEFINER"));
}

#[test]
fn slot_writer_fence_migration_is_atomic_preflighted_and_comment_free() {
    assert!(!MIGRATION.contains("--"));
    assert!(!MIGRATION.contains("/*"));
    assert!(!MIGRATION.contains("//"));
    assert!(!MIGRATION.contains("DISABLE TRIGGER"));
    assert!(!MIGRATION.contains("session_replication_role"));
    let writer_barrier = MIGRATION
        .find("pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)")
        .unwrap();
    let table_barrier = MIGRATION.find("LOCK TABLE").unwrap();
    let preflight = MIGRATION.find("DO $preflight$").unwrap();
    let schema_change = MIGRATION
        .find("ALTER TABLE public.runtime_drain_intents_v2")
        .unwrap();
    assert!(writer_barrier < table_barrier);
    assert!(table_barrier < preflight);
    assert!(preflight < schema_change);
    assert!(MIGRATION.contains("IN ACCESS EXCLUSIVE MODE"));
    for relation in [
        "public.automation_installations",
        "public.runtime_deployments",
        "public.runtime_serving_leases",
        "public.runtime_product_operations_v2",
        "public.runtime_drain_intents_v2",
    ] {
        assert!(MIGRATION[..preflight].contains(relation), "{relation}");
    }
    let preflight_body = dollar_block("preflight");
    for required in [
        "public.runtime_slot_writer_fences_v2",
        CREATE_IDENTITY,
        LOCK_IDENTITY,
        BEGIN_UNSAFE_IDENTITY,
        MARK_DRAIN_IDENTITY,
        INSTALLATION_TRIGGER_IDENTITY,
        "FROM public.runtime_product_operations_v2",
        "FROM public.runtime_drain_intents_v2",
        "runtime_slot_writer_fence_preflight_drift",
        "331a95180a75109385566b0b1b0659e247e5619cf02e2f61ee89904a2751856b",
        "3e2d46d692daf8bd9cff68f00459f00f6b8bf314378a663727b94493d7e45279",
    ] {
        assert!(preflight_body.contains(required), "{required}");
    }
}

#[test]
fn fence_schema_has_canonical_identity_pending_uniqueness_and_composite_links() {
    let table = MIGRATION
        .split("CREATE TABLE public.runtime_slot_writer_fences_v2 (")
        .nth(1)
        .unwrap()
        .split("\n);")
        .next()
        .unwrap();
    for required in [
        "CONSTRAINT runtime_slot_writer_fences_v2_pkey PRIMARY KEY (\n        slot_guild_id,\n        slot_ruleset_key\n    )",
        "CONSTRAINT runtime_slot_writer_fences_v2_installation_fk FOREIGN KEY (\n        slot_guild_id,\n        slot_ruleset_key\n    ) REFERENCES public.automation_installations (\n        discord_guild_id,\n        ruleset_key\n    ) ON DELETE RESTRICT",
        "CONSTRAINT runtime_slot_writer_fences_v2_pending_fk FOREIGN KEY (\n        pending_drain_intent_id,\n        pending_product_operation_id,\n        pending_tenant_id,\n        pending_installation_id,\n        pending_deployment_id,\n        slot_guild_id,\n        slot_ruleset_key,\n        pending_expected_revision\n    ) REFERENCES public.runtime_drain_intents_v2 (\n        drain_intent_id,\n        product_operation_id,\n        tenant_id,\n        installation_id,\n        deployment_id,\n        slot_guild_id,\n        slot_ruleset_key,\n        expected_revision\n    ) ON DELETE RESTRICT DEFERRABLE INITIALLY DEFERRED",
        "runtime_slot_writer_fences_v2_pending_intent_unique UNIQUE",
        "runtime_slot_writer_fences_v2_pending_product_unique UNIQUE",
        "writer_epoch BETWEEN 1 AND 9223372036854775807",
        "pending_drain_intent_id IS NULL",
        "pending_drain_intent_id IS NOT NULL",
        "pending_expected_revision\n                BETWEEN 1 AND 9223372036854775807",
        "pg_catalog.isfinite(pending_marked_at)",
        "pg_catalog.isfinite(updated_at)",
    ] {
        assert!(table.contains(required), "{required}");
    }
    assert!(MIGRATION.contains(
        "runtime_drain_intents_v2_fence_identity_unique UNIQUE (\n    drain_intent_id,\n    product_operation_id,\n    tenant_id,\n    installation_id,\n    deployment_id,\n    slot_guild_id,\n    slot_ruleset_key,\n    expected_revision\n)"
    ));
    assert!(MIGRATION.contains(
        "CREATE UNIQUE INDEX runtime_drain_intents_v2_one_pending_per_slot\nON public.runtime_drain_intents_v2 (\n    slot_guild_id,\n    slot_ruleset_key\n)\nWHERE intent_state = 'pending'"
    ));
    assert!(!table.contains("ON DELETE CASCADE"));
    assert!(!table.contains("ON UPDATE CASCADE"));
}

#[test]
fn every_existing_and_future_installation_gets_exactly_one_clear_fence() {
    let backfill = MIGRATION
        .split("INSERT INTO public.runtime_slot_writer_fences_v2 (")
        .nth(1)
        .unwrap()
        .split("CREATE FUNCTION public.reject_runtime_slot_writer_fence_mutation_v2()")
        .next()
        .unwrap();
    assert!(backfill.contains("FROM public.automation_installations AS installation"));
    assert!(backfill.contains(
        "SELECT\n    installation.discord_guild_id,\n    installation.ruleset_key,\n    1,\n    NULL,\n    NULL,\n    NULL,\n    NULL,\n    NULL,\n    NULL,\n    NULL,\n    pg_catalog.clock_timestamp()"
    ));
    assert!(backfill.contains("ORDER BY installation.discord_guild_id, installation.ruleset_key"));
    assert!(!backfill.contains("\nWHERE "));
    assert!(!backfill.contains("ON CONFLICT"));
    let trigger = function_body(
        "CREATE FUNCTION starring_runtime_private_v2.\
starring_runtime_slot_writer_fence_installation_insert_v2()",
    );
    assert!(trigger.contains(
        "PERFORM starring_runtime_private_v2.starring_runtime_slot_writer_fence_create_v2(\n        NEW.discord_guild_id,\n        NEW.ruleset_key\n    )"
    ));
    assert!(MIGRATION.contains(
        "CREATE TRIGGER automation_installations_create_runtime_slot_writer_fence_v2\nAFTER INSERT ON public.automation_installations\nFOR EACH ROW"
    ));
    assert!(MIGRATION.contains(INSTALLATION_TRIGGER_IDENTITY));
}

#[test]
fn mutation_trigger_is_one_shot_action_and_identity_bound() {
    let body =
        function_body("CREATE FUNCTION public.reject_runtime_slot_writer_fence_mutation_v2()");
    let settings = [
        "starring.runtime_slot_writer_fence_action_v2",
        "starring.runtime_slot_writer_fence_slot_guild_id_v2",
        "starring.runtime_slot_writer_fence_slot_ruleset_key_v2",
        "starring.runtime_slot_writer_fence_expected_epoch_v2",
        "starring.runtime_slot_writer_fence_drain_intent_id_v2",
        "starring.runtime_slot_writer_fence_product_operation_id_v2",
        "starring.runtime_slot_writer_fence_tenant_id_v2",
        "starring.runtime_slot_writer_fence_installation_id_v2",
        "starring.runtime_slot_writer_fence_deployment_id_v2",
        "starring.runtime_slot_writer_fence_expected_revision_v2",
        "starring.runtime_slot_writer_fence_marked_at_v2",
    ];
    for setting in settings {
        assert!(body.matches(setting).count() >= 4, "{setting}");
    }
    for required in [
        "TG_OP = 'INSERT'\n        AND gate_action = 'create'",
        "TG_OP = 'UPDATE'\n        AND gate_action = 'advance'",
        "TG_OP = 'UPDATE'\n        AND gate_action = 'mark_drain'",
        "gate_slot_guild_id = OLD.slot_guild_id",
        "gate_slot_ruleset_key = OLD.slot_ruleset_key",
        "gate_expected_epoch = OLD.writer_epoch::TEXT",
        "gate_drain_intent_id = NEW.pending_drain_intent_id",
        "gate_product_operation_id = NEW.pending_product_operation_id",
        "gate_tenant_id = NEW.pending_tenant_id",
        "gate_installation_id = NEW.pending_installation_id",
        "gate_deployment_id = NEW.pending_deployment_id",
        "gate_expected_revision = NEW.pending_expected_revision::TEXT",
        "gate_marked_at = NEW.pending_marked_at::TEXT",
        "NEW.writer_epoch = OLD.writer_epoch + 1",
        "OLD.pending_drain_intent_id IS NULL",
        "pg_catalog.set_config(setting_name, '', TRUE)",
        "runtime_slot_writer_fence_mutation_rejected",
    ] {
        assert!(body.contains(required), "{required}");
    }
    assert_eq!(
        body.matches("FOREACH setting_name IN ARRAY ARRAY[").count(),
        3
    );
    assert_eq!(body.matches("RETURN NEW;").count(), 3);
    assert!(MIGRATION
        .contains("BEFORE INSERT OR UPDATE OR DELETE ON public.runtime_slot_writer_fences_v2"));
    assert!(MIGRATION.contains("BEFORE TRUNCATE ON public.runtime_slot_writer_fences_v2"));
}

#[test]
fn private_helpers_lock_validate_and_physically_advance_the_epoch() {
    let create_section = function_section(
        "CREATE FUNCTION starring_runtime_private_v2.\
starring_runtime_slot_writer_fence_create_v2(",
    );
    let lock_section = function_section(
        "CREATE FUNCTION starring_runtime_private_v2.\
starring_runtime_slot_writer_fence_lock_v2(",
    );
    let begin_section = function_section(
        "CREATE FUNCTION starring_runtime_private_v2.\
starring_runtime_slot_writer_fence_begin_unsafe_v2(",
    );
    let mark_section = function_section(
        "CREATE FUNCTION starring_runtime_private_v2.\
starring_runtime_slot_writer_fence_mark_drain_v2(",
    );
    let trigger_section = function_section(
        "CREATE FUNCTION starring_runtime_private_v2.\
starring_runtime_slot_writer_fence_installation_insert_v2()",
    );
    assert_invoker_helper(create_section, true);
    assert_invoker_helper(lock_section, true);
    assert_invoker_helper(begin_section, true);
    assert_invoker_helper(mark_section, true);
    assert_invoker_helper(trigger_section, false);

    let create = create_section.split("AS $function$").nth(1).unwrap();
    assert!(create.contains("FROM public.automation_installations AS installation"));
    assert!(create.contains("INSERT INTO public.runtime_slot_writer_fences_v2"));
    assert!(create.contains("RETURNING writer_epoch INTO next_epoch"));
    assert!(create.contains("runtime_slot_writer_fence_gate_consumption_invalid"));

    let lock = lock_section.split("AS $function$").nth(1).unwrap();
    let row_read = lock
        .find("FROM public.runtime_slot_writer_fences_v2 AS fence")
        .unwrap();
    let row_lock = lock.find("FOR UPDATE").unwrap();
    let drain_check = lock
        .find("FROM public.runtime_drain_intents_v2 AS drain")
        .unwrap();
    assert!(row_read < row_lock);
    assert!(row_lock < drain_check);
    assert!(lock.contains("AND drain.intent_state = 'pending'"));
    assert!(lock.contains("fence_row.pending_drain_intent_id IS NULL"));
    assert!(lock.contains("AND EXISTS ("));
    assert!(lock.contains("runtime_execution_product_drain_state_invalid"));

    let begin = begin_section.split("AS $function$").nth(1).unwrap();
    let begin_gate = begin.find("'advance'").unwrap();
    let begin_update = begin
        .find("UPDATE public.runtime_slot_writer_fences_v2 AS fence")
        .unwrap();
    let begin_increment = begin
        .find("SET writer_epoch = fence.writer_epoch + 1")
        .unwrap();
    let begin_returning = begin
        .find("RETURNING fence.writer_epoch INTO next_epoch")
        .unwrap();
    assert!(begin_gate < begin_update);
    assert!(begin_update < begin_increment);
    assert!(begin_increment < begin_returning);
    assert!(begin.contains("AND fence.writer_epoch = requested_expected_epoch"));
    assert!(begin.contains("AND fence.pending_drain_intent_id IS NULL"));
    assert!(begin.contains("AND NOT EXISTS ("));
    assert!(begin.contains("ELSIF EXISTS ("));
    assert!(begin.contains("runtime_execution_product_drain_state_invalid"));
    assert!(begin.contains("runtime_execution_product_drain_pending"));
    assert!(begin.contains("runtime_execution_slot_writer_epoch_stale"));

    let mark = mark_section.split("AS $function$").nth(1).unwrap();
    let exact_pending_root = mark
        .find("FROM public.runtime_drain_intents_v2 AS drain")
        .unwrap();
    let mark_gate = mark.find("'mark_drain'").unwrap();
    let mark_update = mark
        .find("UPDATE public.runtime_slot_writer_fences_v2 AS fence")
        .unwrap();
    let mark_increment = mark
        .find("SET writer_epoch = fence.writer_epoch + 1")
        .unwrap();
    let mark_returning = mark
        .find("RETURNING fence.writer_epoch INTO next_epoch")
        .unwrap();
    assert!(exact_pending_root < mark_gate);
    assert!(mark_gate < mark_update);
    assert!(mark_update < mark_increment);
    assert!(mark_increment < mark_returning);
    for required in [
        "drain.drain_intent_id = requested_drain_intent_id",
        "drain.product_operation_id\n                    = requested_product_operation_id",
        "drain.tenant_id = requested_tenant_id",
        "drain.installation_id = requested_installation_id",
        "drain.deployment_id = requested_deployment_id",
        "drain.slot_guild_id = requested_slot_guild_id",
        "drain.slot_ruleset_key = requested_slot_ruleset_key",
        "drain.expected_revision = requested_expected_revision",
        "drain.intent_state = 'pending'",
        "pending_drain_intent_id = requested_drain_intent_id",
        "pending_product_operation_id = requested_product_operation_id",
        "pending_tenant_id = requested_tenant_id",
        "pending_installation_id = requested_installation_id",
        "pending_deployment_id = requested_deployment_id",
        "pending_expected_revision = requested_expected_revision",
        "pending_marked_at = mark_clock",
        "AND fence.writer_epoch = requested_expected_epoch",
        "AND fence.pending_drain_intent_id IS NULL",
    ] {
        assert!(mark.contains(required), "{required}");
    }
}

#[test]
fn first_apply_patch_locks_fence_before_deployment_and_marks_after_both_roots() {
    let patch = dollar_block("patch_first_apply");
    for drift in [
        "runtime_slot_writer_fence_first_apply_declaration_drift",
        "runtime_slot_writer_fence_first_apply_lock_drift",
        "runtime_slot_writer_fence_first_apply_pair_drift",
        "runtime_slot_writer_fence_first_apply_conflict_drift",
        "runtime_slot_writer_fence_first_apply_mark_drift",
    ] {
        assert!(patch.contains(drift), "{drift}");
    }
    let lock_patch = patch
        .split("next_fragment :=\n        '    SELECT fence.*'")
        .nth(1)
        .unwrap()
        .split("runtime_slot_writer_fence_first_apply_lock_drift")
        .next()
        .unwrap();
    let fence_lock = lock_patch
        .find("starring_runtime_slot_writer_fence_lock_v2")
        .unwrap();
    let deployment_lock = lock_patch.find("'    SELECT deployment.*'").unwrap();
    assert!(fence_lock < deployment_lock);
    assert!(lock_patch.contains("requested_slot_guild_id"));
    assert!(lock_patch.contains("requested_slot_ruleset_key"));

    let product_insert = FIRST_APPLY_MIGRATION
        .find("INSERT INTO public.runtime_product_operations_v2")
        .unwrap();
    let drain_insert = FIRST_APPLY_MIGRATION
        .find("INSERT INTO public.runtime_drain_intents_v2")
        .unwrap();
    let inserted_outcome = FIRST_APPLY_MIGRATION
        .find("outcome_name := 'inserted'")
        .unwrap();
    assert!(product_insert < drain_insert);
    assert!(drain_insert < inserted_outcome);
    assert_eq!(
        FIRST_APPLY_MIGRATION
            .matches("outcome_name := 'inserted'")
            .count(),
        1
    );
    assert!(patch.contains("previous_fragment :=\n        '    outcome_name := ''inserted'';'"));
    let mark_patch = patch
        .split(
            "next_fragment :=\n        '    PERFORM \
starring_runtime_private_v2.starring_runtime_slot_writer_fence_mark_drain_v2('",
        )
        .nth(1)
        .unwrap()
        .split("runtime_slot_writer_fence_first_apply_mark_drift")
        .next()
        .unwrap();
    let mark_end = mark_patch.find("'    );'").unwrap();
    let patched_outcome = mark_patch
        .find("'    outcome_name := ''inserted'';'")
        .unwrap();
    assert!(mark_end < patched_outcome);
    for argument in [
        "requested_slot_guild_id",
        "requested_slot_ruleset_key",
        "slot_fence_row.writer_epoch",
        "requested_intent_id",
        "requested_operation_id",
        "requested_tenant_id",
        "requested_installation_id",
        "requested_deployment_id",
        "requested_expected_revision",
    ] {
        assert!(mark_patch.contains(argument), "{argument}");
    }
}

#[test]
fn first_apply_replay_requires_fence_root_symmetry_and_conflict_is_closed() {
    let patch = dollar_block("patch_first_apply");
    for pair in [
        "slot_fence_row.pending_drain_intent_id",
        "drain_row.drain_intent_id",
        "slot_fence_row.pending_product_operation_id",
        "drain_row.product_operation_id",
        "slot_fence_row.pending_tenant_id",
        "drain_row.tenant_id",
        "slot_fence_row.pending_installation_id",
        "drain_row.installation_id",
        "slot_fence_row.pending_deployment_id",
        "drain_row.deployment_id",
        "slot_fence_row.pending_expected_revision",
        "drain_row.expected_revision",
        "outcome_name := ''persistence_corrupt''",
    ] {
        assert!(patch.contains(pair), "{pair}");
    }
    let conflict_patch = patch
        .split(
            "next_fragment :=\n        '    IF \
slot_fence_row.pending_drain_intent_id IS NOT NULL THEN'",
        )
        .nth(1)
        .unwrap()
        .split("runtime_slot_writer_fence_first_apply_conflict_drift")
        .next()
        .unwrap();
    let conflict = conflict_patch
        .find("'        outcome_name := ''slot_conflict'';'")
        .unwrap();
    let revision_check = conflict_patch
        .find("'    IF deployment_row.revision'")
        .unwrap();
    assert!(conflict < revision_check);
    let conflict_fragment = &conflict_patch[conflict..revision_check];
    for required in [
        "'        locked_snapshot := NULL;'",
        "'        observed_at := NULL;'",
        "'        RETURN NEXT;'",
        "'        RETURN;'",
    ] {
        assert!(conflict_fragment.contains(required), "{required}");
    }
    assert!(patch.contains("pg_catalog.replace(definition, previous_fragment, '')"));
    assert!(patch.contains("EXECUTE definition;"));
}

#[test]
fn deferred_triggers_enforce_bidirectional_pending_symmetry() {
    let section =
        function_section("CREATE FUNCTION public.validate_runtime_slot_writer_fence_symmetry_v2()");
    assert!(section.contains("SECURITY DEFINER"));
    assert!(section.contains("SET search_path = pg_catalog"));
    assert!(section.contains("VOLATILE"));
    assert!(section.contains("PARALLEL UNSAFE"));
    let body = section.split("AS $function$").nth(1).unwrap();
    for required in [
        "TG_RELID = pg_catalog.to_regclass(\n            'public.runtime_slot_writer_fences_v2'",
        "IF TG_OP = 'DELETE' THEN",
        "drain.intent_state = 'pending'",
        "IF NEW.pending_drain_intent_id IS NULL THEN",
        "drain.drain_intent_id = NEW.pending_drain_intent_id",
        "drain.product_operation_id\n                = NEW.pending_product_operation_id",
        "drain.tenant_id = NEW.pending_tenant_id",
        "drain.installation_id = NEW.pending_installation_id",
        "drain.deployment_id = NEW.pending_deployment_id",
        "drain.slot_guild_id = NEW.slot_guild_id",
        "drain.slot_ruleset_key = NEW.slot_ruleset_key",
        "drain.expected_revision = NEW.pending_expected_revision",
        "IF TG_OP <> 'DELETE' AND NEW.intent_state = 'pending' THEN",
        "fence.pending_drain_intent_id = NEW.drain_intent_id",
        "IF TG_OP <> 'INSERT'",
        "fence.pending_drain_intent_id = OLD.drain_intent_id",
        "runtime_slot_writer_fence_symmetry_invalid",
    ] {
        assert!(body.contains(required), "{required}");
    }
    for trigger in [
        "CREATE CONSTRAINT TRIGGER runtime_slot_writer_fences_v2_assert_pending_symmetry\nAFTER INSERT OR UPDATE OR DELETE ON public.runtime_slot_writer_fences_v2\nDEFERRABLE INITIALLY DEFERRED\nFOR EACH ROW\nEXECUTE FUNCTION public.validate_runtime_slot_writer_fence_symmetry_v2()",
        "CREATE CONSTRAINT TRIGGER runtime_drain_intents_v2_assert_slot_writer_fence_symmetry\nAFTER INSERT OR UPDATE OR DELETE ON public.runtime_drain_intents_v2\nDEFERRABLE INITIALLY DEFERRED\nFOR EACH ROW\nEXECUTE FUNCTION public.validate_runtime_slot_writer_fence_symmetry_v2()",
    ] {
        assert!(MIGRATION.contains(trigger));
    }
}

#[test]
fn fence_surface_is_owner_only_and_does_not_grow_public_capabilities() {
    assert!(!MIGRATION.contains("GRANT "));
    assert!(!MIGRATION.contains("ALTER DEFAULT PRIVILEGES"));
    assert!(MIGRATION.contains(
        "REVOKE ALL PRIVILEGES ON TABLE\n    public.runtime_slot_writer_fences_v2\nFROM PUBLIC"
    ));
    for identity in [
        "public.reject_runtime_slot_writer_fence_mutation_v2()",
        "public.validate_runtime_slot_writer_fence_symmetry_v2()",
        CREATE_IDENTITY,
        LOCK_IDENTITY,
        BEGIN_UNSAFE_IDENTITY,
        MARK_DRAIN_IDENTITY,
        INSTALLATION_TRIGGER_IDENTITY,
    ] {
        assert!(MIGRATION.contains(identity), "{identity}");
    }
    assert_eq!(
        MIGRATION
            .matches("REVOKE ALL PRIVILEGES ON FUNCTION")
            .count(),
        7
    );
    let postflight = dollar_block("postflight");
    for required in [
        "privilege.grantee <> common_owner",
        "privilege.grantor <> common_owner",
        "privilege.privilege_type <> 'EXECUTE'",
        "privilege.is_grantable",
        "relation.relowner <> common_owner",
        "relation.relrowsecurity",
        "relation.relforcerowsecurity",
        "runtime_slot_writer_fence_postflight_drift",
    ] {
        assert!(postflight.contains(required), "{required}");
    }
}

#[test]
fn manifest_readiness_and_rust_pins_cover_the_new_private_contract() {
    for required in [
        "RETURN observed_count = 623",
        "ce1e493041abc52b6f4073da976a99b547b32a92d7ff171b64eef791354ff491",
        "223a7d5a5aba3e418ed310c4cffa8271193af158f12729f74ad85be97123c292",
        MIGRATION_READINESS_DIGEST,
        "356::BIGINT",
        "ca4d76873d9256406baaad080943a78b7a6eeeae409ad67e8dc896f0a237642a",
        "5fe0365d0cb4912a01778f3d30a2d649a40e82c5b964ba9e2e7e1901e79eb109",
        EXACT_TARGET_READINESS_DIGEST,
        "471::BIGINT",
        "1b476578005a17dadfa9a6f3d26f966e929af5909cdc5097eb6a63050ec310fa",
        "14a0c119d8fa0b7a85b72509df29156a6c869b5e3f240bc8fffc89fd1a86c4c9",
        SERVING_READINESS_DIGEST,
        "runtime_slot_writer_fence_shared_manifest_drift",
        "runtime_slot_writer_fence_shared_readiness_drift",
        "runtime_slot_writer_fence_manifest_relation_drift",
        "runtime_slot_writer_fence_manifest_function_drift",
        "runtime_slot_writer_fence_manifest_expectation_drift",
        "runtime_slot_writer_fence_readiness_relation_drift",
        "runtime_slot_writer_fence_readiness_protected_drift",
        "runtime_slot_writer_fence_readiness_private_acl_drift",
        "runtime_slot_writer_fence_readiness_manifest_digest_drift",
        "runtime_slot_writer_fence_postflight_drift",
        FENCE_TABLE,
        CREATE_IDENTITY,
        LOCK_IDENTITY,
        BEGIN_UNSAFE_IDENTITY,
        MARK_DRAIN_IDENTITY,
        INSTALLATION_TRIGGER_IDENTITY,
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    for placeholder in [
        "__MANIFEST",
        "__READINESS",
        "__POSTFLIGHT",
        "__FUNCTION",
        "__DIGEST",
    ] {
        assert!(!MIGRATION.contains(placeholder), "{placeholder}");
    }
    for source in [CONTRACT_SOURCE, DATABASE_SOURCE, SECURITY_SUPPORT_SOURCE] {
        assert!(source.contains(CURRENT_READINESS_DIGEST));
        assert!(
            !source.contains("3e2d46d692daf8bd9cff68f00459f00f6b8bf314378a663727b94493d7e45279")
        );
    }
    assert!(EXACT_TARGET_DATABASE_SOURCE.contains(EXACT_TARGET_READINESS_DIGEST));
    assert!(SERVING_DATABASE_SOURCE.contains(SERVING_READINESS_DIGEST));
    let postflight = dollar_block("postflight");
    assert!(postflight.contains(
        "manifest_digest\n            <> \
'223a7d5a5aba3e418ed310c4cffa8271193af158f12729f74ad85be97123c292'"
    ));
    assert!(postflight.contains(&format!(
        "readiness_digest\n            <> '{MIGRATION_READINESS_DIGEST}'"
    )));
    assert!(postflight.contains("installation_count <> fence_count"));
    assert!(postflight.contains("unmatched_installation_count <> 0"));
    assert!(postflight.contains("unmatched_fence_count <> 0"));
    assert!(postflight.contains(EXACT_TARGET_READINESS_DIGEST));
    assert!(postflight.contains(SERVING_READINESS_DIGEST));
}
