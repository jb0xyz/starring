# Product Apply executor capability

Date: 2026-07-19

## Objective

Move product Apply behind one independently deployable PostgreSQL capability
without changing its domain contract, atomicity, retry rules, deterministic
Rust preparation, runtime deployment request, receipt, audit, replay, or drift
supersession semantics.

The completed component must run with a direct-login role that cannot read or
mutate protected relations, execute decision or approval capabilities, create
database objects, inherit another role, or grant its capability onward. A
compromised Apply credential can invoke only the exact Apply protocol and
cannot bypass the server-bound request, product session, fresh Discord
authority, payload, revision, baseline, artifact, or idempotency checks encoded
by that protocol.

This component is necessary but not sufficient for production ingress.
Promotion publication, deployment-status reads, runtime convergence, rejection
persistence, final relation ACL sealing, and production composition readiness
remain separate gates.

## Preserved protocol

Apply remains one bounded `SERIALIZABLE, READ WRITE` transaction:

1. `starring_product_apply_lock_v1` locks and validates the exact request,
   authorization, authority generation, baseline, quorum, receipt, and runtime
   lane. It returns a replay, terminal supersession, closed error, or a locked
   server projection.
2. A target-artifact capability reloads the exact activation and immutable
   RuleSet artifact under the same transaction snapshot and lock domain.
3. Rust parses the RuleSet, enforces the supported schema version, recomputes
   structural and content integrity, and deterministically prepares the active
   pointer and runtime deployment snapshot.
4. `starring_product_apply_finalize_v1` revalidates the lock projection and
   atomically writes the pointer, decision state, deployment request, receipt,
   alias, audit event, and receipt-audit evidence.
5. Rust validates the returned projection and commits once.

The transaction retries at most once, and only after PostgreSQL proves rollback
with `40001` or `40P01`. Connection loss, protocol failure, or an uncertain
commit result is never replayed automatically and remains `Indeterminate`.

The Rust preparation step intentionally stays outside PostgreSQL. It is pure,
versioned application logic and must not be duplicated in PL/pgSQL. It executes
while database locks are held to preserve the existing safety proof. Lock-hold
latency must be measured before moving preparation outside the transaction; any
such optimization requires a separate persisted intent and compare-and-swap
design.

## External capability manifest

The Apply direct-login role executes exactly five functions:

1. `starring_product_apply_executor_database_identity_v1()`;
2. `starring_product_apply_lock_v1(...)` with its existing 30 inputs and
   eight-column result;
3. `starring_product_apply_target_artifact_v1(text,text,text,text,bytea,text,text)`;
4. `starring_product_apply_finalize_v1(...)` with its existing 35 inputs and
   seven-column result;
5. `starring_product_apply_keyring_coverage_v1(text[],text[])`.

All five are owned by the common restricted `NOLOGIN` product owner and are
ordinary `VOLATILE`, `STRICT`, `PARALLEL UNSAFE`, `SECURITY DEFINER` functions
with exactly `search_path=pg_catalog`. Set-returning functions declare `ROWS 1`.
`PUBLIC`, named non-owner, inherited, and grant-option execution are absent
after migration. The application role receives no execution on internal helper
or trigger functions.

The 30- and 35-input interfaces are intentionally retained in this security
slice. Replacing them with a composite request type would couple capability
hardening to a new serialization and migration contract. A smaller V2 protocol
can be designed after the exact V1 boundary is deployed and measured.

## Target artifact capability

`starring_product_apply_target_artifact_v1` accepts:

1. tenant ID;
2. installation ID;
3. promotion ID;
4. principal ID;
5. opaque 32-byte product-session digest;
6. acting Discord user ID;
7. guild ID.

It returns at most two internal rows with:

- schema version;
- definition, or `NULL` when its serialized representation exceeds 512 KiB;
- persisted content hash;
- canonical content hash.

The function accepts only canonical identifiers, a `product_authoring`
activation, the exact promotion/tenant/installation/guild scope, the exact
principal-to-Discord-user binding, and the exact product session. Tenant,
installation, principal, and session activity are evaluated against the
transaction timestamp so a successful lock cannot become a different public
error merely because wall-clock time advances during deterministic
preparation. The lock function remains the authoritative fresh authorization
check.

Malformed, missing, wrong-scope, wrong-user, wrong-session, and non-product
inputs return zero rows without a distinguishing reason. Corrupt artifacts are
still projected so Rust can preserve `TargetCorrupt`; the function must not
turn persisted corruption into `NotFound`. Rust applies an outer `LIMIT 2`,
maps zero rows to the existing corrupt-target path after a ready or replay
lock, accepts exactly one row, and treats multiple rows as a redacted
persistence failure.

The function takes `FOR SHARE` locks on the activation and RuleSet version,
preserving the current direct query's lock and snapshot behavior.

## Apply keyring coverage

`starring_product_apply_keyring_coverage_v1` validates one to eight unique,
canonical key IDs and matching unique lowercase SHA-256 key fingerprints. It
returns one closed outcome:

- `ok` when every live `product_apply_v1` receipt has an alias covered by the
  supplied ID/fingerprint pairs;
- `idempotency_keyring_incomplete` when coverage is incomplete;
- `invalid_input` for every malformed candidate set.

Apply readiness invokes this capability with the configured keyring. It does
not rely on the approval role or on approval readiness having run. The existing
public combined coverage check remains compatible during this slice; final
process readiness requires every component check.

## Protected relation manifest

The complete direct and transitive Apply boundary contains 18 ordinary,
non-RLS relations with one common owner:

1. `product_control_plane_identity`;
2. `activation_requests`;
3. `activation_request_approvals`;
4. `authoring_promotions`;
5. `product_tenants`;
6. `automation_installations`;
7. `automation_installation_authority_versions`;
8. `product_principals`;
9. `product_auth_sessions`;
10. `product_action_receipts`;
11. `product_action_receipt_idempotency_aliases`;
12. `product_audit_events`;
13. `product_action_receipt_audit_evidence`;
14. `automation_ruleset_activations`;
15. `automation_ruleset_versions`;
16. `runtime_deployments`;
17. `runtime_serving_leases`;
18. `runtime_attestations`.

The last two are transitive reads in runtime deployment projection validation.
The dedicated Apply caller has no table, view, sequence, or column privilege on
this manifest. Transitional grants required by other unfinished adapters are
not silently revoked by this migration, but they keep component readiness red
until the final sealing rollout.

## Internal routine and trigger boundary

The common owner alone executes the internal Apply graph, including:

- `starring_product_apply_lock_core_v1`;
- `starring_product_apply_authority_projection_v1`;
- `starring_product_ruleset_slot_exact_v1`;
- `starring_runtime_desired_target_digest_v1`;
- `starring_runtime_lock_current_authority`;
- `starring_runtime_current_mutation_clock`;
- all activation, pointer, receipt, audit, and runtime deployment trigger
  functions reached by lock or finalize.

The Apply migration and readiness contract reuse the exact approval support
trigger graph and add the exact pointer/runtime mutation graph. This includes
the product-slot constraint trigger on `automation_ruleset_activations` and all
four user triggers on `runtime_deployments`:

- `runtime_deployments_validate_projection`;
- `runtime_deployments_policy_shadow_guard`;
- `runtime_deployments_guard_ruleset_artifact_transition`;
- `runtime_deployments_reject_delete`.

The manifest binds trigger name, relation, function, event vector, timing,
row/statement level, enabled state, constraint identity, deferrability,
initially-deferred state, `WHEN` expression, update-column vector, argument
count and bytes, parent relation, and transition-table bindings. Missing,
extra, disabled, repointed, narrowed, or widened triggers fail closed.

Internal routines have exact owner, language, kind, volatility, strictness,
parallel mode, security mode, result, row estimate, and fixed search path.
They expose no effective non-owner execution. The runtime mutation-clock
function's transaction-local gate and the authority-lock function's semantics
remain unchanged.

## Transaction configuration

Every Apply attempt configures, transaction-locally:

- `SERIALIZABLE, READ WRITE`;
- bounded statement timeout;
- bounded lock timeout;
- bounded idle-in-transaction timeout;
- `search_path=pg_catalog`;
- `quote_all_identifiers=off`.

No step may start a second transaction, use another pool, or move artifact
loading outside the locked snapshot. Failure before commit always rolls back.
Only a proven rolled-back serialization/deadlock failure enters the bounded
retry loop.

## Readiness contract

`verify_apply_executor_readiness` verifies, in order:

1. exact metadata and owner for the five caller functions;
2. exact shape, owner, RLS state, and global table/column ACL state for all 18
   relations;
3. exact internal helper and trigger semantic manifests;
4. trusted `public` schema and the caller's exact executable allowlist;
5. restricted direct-login role properties and topology identity;
6. Apply-only keyring coverage;
7. bounded rollback-only lock, artifact, and finalizer probes.

The lock probe uses canonical non-secret values with an invalid capability and
must return exactly one `invalid_input` row. The artifact probe uses an
impossible canonical scope/session tuple and must return zero rows. The
finalizer probe has no valid lock token and must return exactly one row:
`lock_required, NULL, NULL, FALSE, NULL, NULL, NULL`. The entire functional
probe transaction is rolled back even on success.

These probes prove executable shape and selected closed behavior, not function
body integrity or a successful Apply. Restricted-role end-to-end tests prove a
real Apply, exact replay, terminal supersession, atomic deployment and receipt
evidence, artifact corruption rollback, contention, and stable error mapping.

`verify_product_decision_boundary_readiness` runs the full reader, approval,
and Apply component checks, then requires one logical database UUID/name and
three distinct direct-login roles. The older approval-boundary method remains
for compatibility but is not an ingress gate after this slice.

## Migration and rollout

Migration 022 is atomic and must:

1. preflight all 18 relations, their common owner, RLS state, schema trust, and
   the exact preexisting Apply/helper/trigger graph;
2. require `current_user` to equal the common owner with effective `CREATE` on
   `public`;
3. reject every same-name overload before creating the artifact and coverage
   functions;
4. normalize external and internal routine metadata, ownership, and ACLs;
5. run data-independent probes and verify the post-migration catalog contract;
6. restore transaction-local deparser settings on success.

The migration creates no environment role and grants no application
capability. Production grants are applied from the reviewed environment
manifest after migration. It strips effective inherited function grants from
the protected routines but does not rewrite `pg_default_acl`; restrictive
default privileges remain a mandatory pre-ingress audit.

This is a stopped-maintenance rollout. The old binary needs direct artifact
table access while the new binary needs the artifact function, and the exact
executable readiness allowlists reject mixed manifests. Drain old processes,
apply migration 022 as the common owner through a temporary audited `SET ROLE`
path, install the new binary and exact five grants, revoke temporary membership,
and run the restricted-role functional suite. Apply component readiness remains
red in an environment that still carries transitional relation grants. Do not
reopen product ingress after this slice alone. Component and aggregate
readiness may turn green, and ingress may reopen, only after every remaining
direct-relation adapter is converted and the final sealing rollout removes
those grants.

Rollback never grants a shared privileged credential. Restore a previous binary
only with its reviewed previous capability manifest and closed ingress. An
uncertain migration or readiness result leaves ingress closed.

## Observability and product limits

Before claiming a commercial Apply SLO, measure production-shaped RuleSets and
receipt history for:

- pool acquire latency;
- lock-function, Rust preparation, artifact, finalizer, commit, and total
  transaction latency;
- p50, p95, and p99 lock-hold time;
- serialization/deadlock retry rate;
- timeout, pool saturation, incomplete coverage, rollback, and indeterminate
  commit counts.

Metric labels are finite operation/result/error-class values. Tenant,
installation, promotion, principal, Discord user, request, digest, deployment,
or receipt values never appear in metric labels or error strings.

Catalog metadata readiness does not attest routine body digests. Body integrity
depends on immutable reviewed migration provenance, migration checksums,
restricted owner and DDL credentials, and schema-change audit evidence until a
separate body-attestation design is accepted.

## Acceptance criteria

- Apply uses only its dedicated pool and exact five-function manifest.
- No Apply Rust query references a protected relation directly.
- Lock, deterministic preparation, artifact validation, finalization, retry,
  replay, supersession, and indeterminate-commit semantics remain unchanged.
- A restricted direct-login role completes a real Apply and exact replay while
  every direct relation, DML, DDL, temporary-object, decision, approval,
  internal-helper, and grant-option path is denied.
- Missing or excessive execution, table or column ACL, role membership, owner,
  RLS, schema, function metadata, trigger, topology, or keyring drift fails
  readiness closed.
- Malformed and scope-mismatched artifact inputs are non-enumerating; corrupt
  artifacts remain `TargetCorrupt` and commit no Apply evidence.
- Migration failures for wrong current role, split ownership, RLS, hostile
  trigger metadata, or routine drift are atomic and leave no function or ACL
  residue.
- Workspace tests, restricted-role PostgreSQL tests, dependency guards, Clippy
  with warnings denied, formatting, source-comment checks, and migration checks
  are green.
