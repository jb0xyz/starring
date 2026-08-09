# Runtime milestone B6 staging end-to-end evidence

Date: 2026-07-31 KST

Status: Phase B functional staging milestone evidence, not commercial runtime
certification

## Final candidate identity

- executable source revision:
  `478088754b00a65a6f8d39882e66dc09718e9c91`
- executable SHA-256:
  `800aaa3cfae105e3c787c311eca998ab589c984471a26c0b370dbf796a43ce6b`
- immutable installed binary:
  `starring-runtime-478088754b00a65a6f8d39882e66dc09718e9c91`
- role-permission correction:
  `d02fbb9a7d29edd5c7ce9f606125e2d22b0412d5`
- periodic teardown supervisor:
  `c5bb6136c1add4506c693400be86da3c062a75e4`
- final gateway-owner boundary correction:
  `478088754b00a65a6f8d39882e66dc09718e9c91`
- migration ledger: 114 entries, latest `202607310021`
- Rust: Cargo `1.97.0`
- PostgreSQL: `16.14`
- host: `Mac16,10`, 10 physical CPU cores, 24 GB memory
- macOS: `26.5.2` build `25F84`

All runtime, Discord interaction, restart, and teardown observations below use
the one executable source revision and binary hash above. The persisted product
flow is the exact durable input continued by that candidate. Earlier `7f07923`
and `d02fbb9` runs were diagnostic predecessors only and are not pooled into
the final runtime acceptance result.

The API and runtime used Keychain-backed credentials. No database password,
Discord token, OAuth secret, worker token, envelope key, complete database URL,
or key material is present in this evidence. API and runtime health listeners
remained loopback-only.

## Scope and safety boundary

The run used:

- the dedicated non-customer staging PostgreSQL database
- installation `installation:starring-smoke-test`
- RuleSet slot `1524810437118525551/starring_smoke_test`
- an isolated per-run Discord resource namespace in the non-customer staging
  guild

The standing staging database, installation, immutable deployment evidence, and
certified base Create panel were retained because the product does not yet have
an installation-retirement or uninstall operation. Every per-run role, channel,
panel, and active instance resource was removed.

This is the explicit Phase B standing-fixture scope. It does not satisfy or
weaken D2. D2 still requires a unique disposable database and resource prefix,
duplicate delivery, injected indeterminate failure, route replacement, gateway
disconnect, total test-resource disposal, and zero unresolved operation,
receipt, journal, route, or instance state.

The model participated only in authoring. Promotion, approval, Apply,
deployment convergence, route serving, interaction execution, and teardown
were deterministic. No manual activation, active-pointer mutation, or
smoke-only authority was used.

## Product lifecycle evidence

The authenticated product flow continued the exact stored one-shot generation
proven by the A6 trusted-writer and promotion gate.

| Step | Observed result |
| --- | --- |
| Promotion | `a882658a87b3f69aa56fae5991af78f66a2d6bef6f0e9bd0893ff9633e270932` |
| Preview | `pending_approval`, revision 1 |
| Preview summary | 1 panel, 1 modal, 4 rules, 15 actions, target version 1 |
| Approval | `approved`, revision 2 |
| Apply | `runtime_pending`, replay false |
| Deployment | `dfe295cb12a84a760b0296090a60cd9f3576f6f19354460b99933352646f67e3` |
| Exact target | guild `1524810437118525551`, key `starring_smoke_test`, version 1, runtime generation 1 |
| Terminal staging state | `live` |

The certified standing Create panel remained:

- channel `1524810437667852431`
- message `1532457160003293206`

It remained reachable after the per-run instance was torn down.

## Runtime lifecycle evidence

The final candidate converged and recovered the exact requested deployment
through hydration, panel reconciliation, gateway certification, serving-lease
publication, and Live. Liveness, readiness, and interaction readiness returned
HTTP 200. API liveness and readiness also returned HTTP 200.

The runtime was restarted while the final per-run instance was active. The same
candidate reacquired the canonical owner, reconstructed the exact serving
route, retained the base panel certificate, and recovered the historical
instance against its pinned RuleSet artifact.

An immediate launchd replacement can make one failed-closed startup attempt
while the predecessor's durable owner lease remains authoritative, then
converge on the next bounded attempt. The accepted process returned healthy
within the 90-second staging window and did not expose false readiness or false
Live. This extra retry and exit-code-70 history remains Phase D restart and SLO
work, not a clean-start certificate.

## Final Discord interaction matrix

The real Create-button and modal-submit path produced:

- instance `i_gmfg9jfnwtkr9hdpjxpx4s6p`
- role `1532677575736819845`, `b6-final2 members`
- channel `1532677577355956274`, `study-b6-final2`
- Join panel message `1532677586881085612`
- Help and welcome panel message `1532677583102283776`
- creator principal `1056857223529250906`

| Path | Observed result |
| --- | --- |
| Create button | opened the configured room-name modal |
| Modal submission | completed the deterministic submit rule |
| Create response | `Created b6-final2` |
| Role creation | permissions exactly `0`, managed false |
| Private channel | role allowed `VIEW_CHANNEL` value 1024 |
| Private channel | `@everyone` denied `VIEW_CHANNEL` value 1024 |
| Creator grant | creator initially held the instance role |
| Welcome panel | rendered in the private room with the Help component |
| Join panel | rendered in the bound hub channel with the Join component |
| Registration | persisted the exact role, channel, and two-message manifest |

To exercise a real Join grant after restart, the creator's instance role was
removed once with the staging bot edge. This test-only mutation did not create
or delete a product resource and did not alter product, deployment, route, or
instance authority. The post-restart Join path restored the role and returned
`Joined the study room`. The post-restart Help path returned
`This is a private study room`.

Immediately after those two post-restart interactions, runtime counters showed
two completed interactions, zero in flight, and zero failures or rejections.
The user independently confirmed the Create and modal result, restored Join
grant, and Help response.

## Restart and pinned reconstruction evidence

Before teardown, the least-privilege interaction capability returned:

- the same guild and instance identity
- RuleSet `starring_smoke_test` version 1
- kind `study_room`
- status `active`
- pinned artifact schema 1
- pinned artifact hash
  `fcdd4844bc3f05c1926286eceb792de91a72531d427c93962d065a1979e6278d`
- 4 pinned rules

Its immutable resource manifest was:

```json
{
  "roles": {
    "member_role": "1532677575736819845"
  },
  "channels": {
    "room_channel": "1532677577355956274"
  },
  "messages": {
    "hub_panel": {
      "id": "1532677586881085612",
      "channel": "1524810437667852431"
    },
    "welcome_panel": {
      "id": "1532677583102283776",
      "channel": "1532677577355956274"
    }
  }
}
```

The same final candidate recovered this pinned artifact and manifest after
restart and successfully dispatched both a historical-instance Join route and
the current static Help route.

## Periodic teardown evidence

The final candidate includes a bounded periodic teardown supervisor that:

- starts only after canonical gateway-owner acquisition and the final
  operation-open check
- shares the same narrow interaction database capability, Discord client, and
  per-instance lock as live dispatch
- performs an immediate scan followed by a 30-second cadence
- bounds scan, item, concurrency, pagination, and shutdown work
- stops before canonical owner release and database-pool closure

After the two verified interactions completed with zero in flight, only the
narrow interaction capability function claimed the instance as `deleting`.
No manual Discord DELETE request was issued for an owned resource. The running
periodic supervisor converged the instance to `deleted` in 12 seconds.

| Resource | Absence proof |
| --- | --- |
| Join panel message | HTTP 404, Discord code 10008 |
| private room channel | HTTP 404, Discord code 10003 |
| Help and welcome panel | HTTP 404, Discord code 10003 because the channel was absent |
| member role | absent from the guild role listing |
| creator membership | instance role absent |
| instance state | `deleted` tombstone with the immutable manifest preserved |

The certified base Create panel remained HTTP 200. Runtime liveness and
readiness remained HTTP 200 after automatic cleanup.

## Final clean-source gates

The gates ran in a detached clean worktree at exactly
`478088754b00a65a6f8d39882e66dc09718e9c91`.

| Gate | Result |
| --- | --- |
| `cargo test --locked --workspace --quiet` | 4,285 passed, 0 failed, 394 ignored |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | passed |
| `cargo fmt --all -- --check` | passed |
| `git diff --check` | passed |

The workspace command did not opt into ignored PostgreSQL targets. The real
staging product, database, gateway, Discord, restart, and teardown path was
exercised separately as recorded above.

An earlier candidate failed the exact gateway-owner staging boundary because a
new supervisor method name contained the prohibited activation term. The method
was renamed without changing behavior, the boundary guard passed, and only the
corrected revision above was installed and accepted.

## Stop boundary

This evidence closes the Phase B functional staging slice. It does not claim
commercial readiness. Durable duplicate-interaction receipts, complete
whole-action-plan preflight, effect journaling, indeterminate-effect
reconciliation, bounded compensation, dedicated live teardown-retry degraded
health projection, restart and failure cohorts, load and soak SLOs,
backup/restore, non-interactive reboot, D2 disposable-database E2E, CI
merge-candidate certification, and merged-main certification remain Phase C
and Phase D work.

Later scope classification: this dated stop boundary records everything that
was still open at B6. The current authoritative completion plan assigns the
exact disposable-guild, external-failure recovery, merge-candidate, and
merged-main cohorts to Backend V1 D1-D4. Sustained load and soak,
disaster-recovery restore, and non-interactive host reboot are separate
production-rollout certificates. This note changes no B6 result or claim.
