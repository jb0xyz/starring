# Runtime Phase D1 restart and failure cohort evidence

Date: 2026-08-01 KST

Status: D1 accepted; D2, D3, and the commercial release certificate remain
open

## Certified boundary

The controlled live cohort used source commit
`b4f2bb09f4997c2fda33ddef6a1175e642ca19ba` and these immutable artifacts:

| Artifact | SHA-256 |
| --- | --- |
| `starring-runtime` | `4a24431cf0a9ca1341acb1ff161bd1c74fa9227f7dff5949fb348dfd188dc23c` |
| `starring-db-bootstrap` | `f38c48df7680f88ec6d0401f0013fb8204a7f6eb68f932ee848fc65aa4080ee4` |

The source-only test-boundary correction is commit
`e4da0097ad7acaab446f5171e4d2526463753104`. It changes test imports and the
dependency inventory, not the production runtime path. The final D3
merge-candidate build and hashes must therefore be generated and certified
independently.

The staging upgrade was serialized with the API, runtime, and tunnel stopped,
zero active database clients, and no prepared transaction. Run
`20260801T133834Z-b4f2bb0-d1` retained a mode-`0600` pre-migration backup at
`~/Library/Application Support/Starring/backups/20260801T133834Z-b4f2bb0-d1/`
with SHA-256
`2e374b053e10cebc89a5cd8e988ab8a9983ab5a145947864b6aeea45e2db31e1`.
The exact bootstrap receipt was:

```text
database=starring_runtime_staging owner=starring_owner migrations=117 relations=198 capability_functions=135
```

The migration-117 effect ACL backfill, restricted response-tail scan, and
redacted effect inspection passed. The inspection returned zero
recovery-required groups.

## Required checkpoints

Each durable checkpoint has an exact deterministic or PostgreSQL restart seam.
The live process cohort is additional evidence for the process boundary; it
does not replace the durable-state tests.

| Checkpoint | Evidence |
| --- | --- |
| Requested | `requested_builder_has_a_stable_v1_digest_and_exact_snapshot` |
| Claimed | `claims_and_failures_persist_exact_convergence_attempts`; `concurrent_claims_start_only_one_convergence_attempt` |
| PreflightReady | `preflight_ready_replay_rechecks_discord_and_stages_without_mutation` |
| ActivationApplying | `durable_phases_map_to_one_allowed_action`; `durable_phase_continuation_never_replays_an_earlier_mutation` |
| ReconcilingPanels | `durable_phases_map_to_one_allowed_action`; `exact_live_status_and_fencing_survive_postgres` |
| AwaitingGatewayReady | `durable_phase_continuation_never_replays_an_earlier_mutation` |
| Certification reserved | `certification_reservation_adapter_is_replay_safe_and_capability_scoped` |
| Certification commit indeterminate | `certification_terminal_ledger_is_immutable_and_reclassifies_only_exact_history`; `only_indeterminate_database_outcomes_receive_exact_finalization` |
| Live before first heartbeat renewal | `exact_start_handshake_returns_ready_and_commanded_stop_returns_authority` |
| Live with fresh serving lease | `serving_mutations_are_replay_safe_bounded_and_least_privilege` plus the live ACK cohort below |
| Draining with active interactions | `predecessor_removal_waits_for_active_interactions_to_reach_zero` |
| Suspended | `startup_suspended_local_execution_progresses_and_replays_exactly_once` |
| Recovery-required interaction | `effect_recovery_fences_active_receipts_and_persists_budget_and_response_replays` |
| Shutdown | `composed_invalidation_seals_every_admission_surface_before_gateway_shutdown` |

## Required failures

| Failure | Closed evidence |
| --- | --- |
| Database unavailable before claim | `database_unavailable_before_claim_has_bounded_retry_and_zero_downstream_work`: exact five-second retry, no convergence or external work |
| Database unavailable before effect | `fenced_persistence_outages_stop_before_the_external_call` |
| Discord unavailable before effect | `discord_preflight_unavailable_after_claim_retains_scope_without_mutation_or_effects` |
| Discord outcome indeterminate | `indeterminate_effect_requires_recovery_without_replay` |
| Gateway disconnect | `disconnect_invalidates_ready_evidence_and_requires_a_new_resume` |
| Owner lease loss | `owner_loss_is_terminal_fail_closed_and_returns_retained_state` |
| Controller lease loss | `wrong_or_expired_lease_fails_closed` |
| Writer-fence change | `slot_writer_fence_physical_epoch_update_aborts_a_stale_serializable_writer` |
| Installation authority rotation | `runtime_authority_tracks_binding_identity_across_policy_rotation` |
| Binding-map change | `preflight_binding_drift_fails_before_mutation` |
| Process kill and restart | controlled live `SIGKILL` cohort below |
| Duplicate HTTP authoring turn | `concurrent_same_idempotency_key_executes_one_model_call_and_replays` |
| Duplicate Discord interaction | `durable_receipt_restricted_role_runs_lifecycle_recovery_and_token_expiry` |

The immutable product path retains no stale writer and opens no false Live.
Indeterminate mutable effects stay durable and blocked instead of replaying;
compensation requires exact identity and preimage evidence and never deletes an
unrelated resource. Retryable controller and recovery paths have fixed next
actions and attempt bounds. Terminal recovery block codes and their required
operator actions are enumerated in the runtime staging operations runbook.

## Controlled live restart cohort

The first healthy start reached loopback liveness and deep readiness in six
seconds. The same process then returned HTTP 200 for both endpoints for 40
samples over 83 seconds without a failure code.

Graceful restart rotated process identity and renewed ingress acknowledgement
without opening readiness early. A forced `SIGKILL` then exercised launchd and
the retained database authority. Stale authority caused bounded fail-closed
successor attempts rather than concurrent serving. The final successor became
healthy in 66 seconds after three observed successor PIDs. Its build revision
matched the immutable candidate, its acknowledgement revision advanced, its
source revision was exactly the current predecessor, and its process hash
rotated.

The recovered process was sampled 40 times over 83 seconds:

| Signal | Result |
| --- | --- |
| Exact successor PID retained | 40/40 |
| `/health/live` HTTP 200 | 40/40 |
| `/health/ready` HTTP 200 | 40/40 |
| Fresh ingress acknowledgement | 40/40 |
| Runtime failure log codes | 0 |

The API and tunnel were then restored. Internal API liveness and readiness
returned HTTP 200 with the configured host, and the public authenticated
`/v1/me` route returned the expected Cloudflare Access redirect. Public health
and root paths remain intentionally unexposed.

## Verification

| Gate | Result |
| --- | --- |
| Runtime worker and `starring-runtime` all targets | passed; no failures; the one pre-existing ignored runtime test remained ignored |
| `starring-runtime` dependency guard | 52 passed, 0 failed |
| Focused runtime Clippy with warnings denied | passed, 0 warnings |
| Runtime formatting and diff checks | passed |
| Convergence PostgreSQL cohort | 22 passed, 0 failed |
| Execution PostgreSQL cohort | 121 passed, 0 failed |
| Serving PostgreSQL cohort | 1 passed, 0 failed |
| Interaction PostgreSQL cohort | 10 passed, 0 failed |
| Migration 117 PostgreSQL 16 correction proof | migration 116 fails with PostgreSQL `42883`; migration 117 repairs the scan and returns each eligible row exactly once |

The disposable database and guild E2E, exact merge-candidate full gate, GitHub
CI, merged-main identity check, and final operations closure remain D2–D4.
This document is D1 evidence, not a commercial release certificate.
