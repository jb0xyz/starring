# Product decision database capability boundaries

Date: 2026-07-19

## Objective

Separate product-decision reads, approval mutations, and apply mutations into
independent PostgreSQL capabilities without changing the application-domain
ports, approval semantics, apply semantics, idempotency, audit evidence, or
deployment safety boundary.

The completed boundary must be suitable for production ingress. Compromise of
one direct-login database credential must not grant another operation family,
direct access to a protected product-control relation, role escalation, schema
mutation, or execution of an unlisted product-control routine.

The approval slice proves only its enumerated 16-relation manifest and routines
in the `public` schema that are either named `starring_*` or are security
definers. Views, sequences, relations outside that manifest, routines in other
schemas, and schema privileges outside `public` remain part of the final
whole-process object and executable manifest. This slice is not that final
gate.

## Current state and staged target

At design acceptance, `PostgresProductDecisions` routed queries, approvals,
apply work, and keyring coverage through one `PgPool`. The implemented store
shape now requires the three pools below, and migrations 019 through 022 place
all three operation families behind exact executable manifests. This completes
the product-decision component boundary, but it is not the final whole-process
capability seal.

The Rust store requires three pools:

| Pool | Final responsibility | Transitional state |
| --- | --- | --- |
| Decision reader | Approval preview and product-status projection | Two exact functions; no direct relation SQL |
| Approval executor | Approval mutation and approval/apply receipt keyring coverage | Three exact functions; no direct relation SQL |
| Apply executor | Apply lock, artifact read, deterministic preparation, and atomic finalization | Five exact functions; deterministic preparation remains pure Rust |

The constructor intentionally has no single-pool production convenience. Tests
that exercise legacy semantics may explicitly provide three clones. Production
composition must provide three independently configured pools and direct-login
roles. A rejection executor is added only when the product rejection adapter
exists; an unused privileged credential is not provisioned in advance.

This work is staged to avoid an upgrade outage. Migrations 019 through 022 do
not silently revoke legacy relation grants needed by other unfinished adapters.
Any non-owner grant on a reader, approval, or Apply protected relation makes the
corresponding readiness gate red. Grants outside those component manifests
remain an explicit whole-service ingress blocker until the remaining adapters
are function-scoped and a later sealing migration removes every non-owner
relation, view, sequence, and column grant in the final process manifest.

## Threat model

The boundary protects against:

- accidental pool wiring across environments or restored database clones;
- theft of one database credential;
- a caller receiving direct table or column privileges on the enumerated
  protected relations;
- `PUBLIC`, named-role, inherited-role, or grant-option privilege drift;
- execution of an unrelated `public.starring_*` function or public
  security-definer routine;
- a login role that can create databases, temporary objects, schemas, or roles;
- ownership drift, owner-role membership, RLS drift, search-path injection, and
  function metadata drift;
- partial migrations that expose a new capability before all prerequisites are
  valid.

The boundary does not make an in-process authorization capability
cryptographically attestable to PostgreSQL. A compromised process that holds a
valid application request and the matching dedicated database credential can
invoke that capability. Request validation, fresh Discord authority evidence,
idempotency digests, and deterministic approval and apply checks remain
mandatory.

## Invariants

All decision pools must:

1. connect to one logical control-plane UUID and one database name;
2. use distinct direct-login roles with `current_user = session_user`;
3. have database `CONNECT` and schema `USAGE` only;
4. have no role membership, owner membership, superuser, `CREATEDB`,
   `CREATEROLE`, replication, or `BYPASSRLS` capability;
5. have no database `CREATE` or `TEMPORARY` and no schema `CREATE`;
6. execute exactly the versioned functions in their capability manifest,
   without grant option;
7. have no direct table or column privilege on protected relations.

Every caller-exposed protected function is `SECURITY DEFINER`, `VOLATILE`,
`STRICT`, `PARALLEL UNSAFE`, fixed to `search_path=pg_catalog`, closed to
`PUBLIC`, owned by the common non-login relation owner, and assigned an explicit
singleton row estimate when set-returning. Internal approval trigger functions
are not caller capabilities. Migration 020 schema-qualifies the one legacy
`authoring_promotions` reference that previously depended on `public` path
resolution, then fixes every internal trigger function to
`search_path=pg_catalog`. The production schema contract separately forbids
schema creation by request roles and every other untrusted principal.
Migrations remove every named non-owner function grant before ownership
normalization. Production grants are always reapplied after migration and
checked before ingress.

## Logical database identity

Three role-specific topology functions expose the existing non-secret UUID in
`product_control_plane_identity`:

- `starring_product_decision_reader_database_identity_v1()`
- `starring_product_approval_executor_database_identity_v1()`
- `starring_product_apply_executor_database_identity_v1()`

Readiness loads each UUID with `current_database()`, `current_user`, and
`session_user`. It requires canonical nonzero lowercase UUID text, one database
name, and three distinct role names. A separately migrated database or an
independent clone is rejected even when its schema and grants match. A failover
member of the same logical database retains the UUID. An independent restored
environment must rotate it before receiving service connections.

## Approval executor slice

The approval pool alone executes:

- `starring_product_approval_executor_database_identity_v1()`;
- `starring_product_approve_v1(...)`;
- `starring_product_approval_keyring_coverage_v1(text[], text[])`.

`starring_product_approve_v1` remains the single atomic approval mutation. Its
28-argument request binding, serializable transaction, advisory key-coverage
lock, authority checks, quorum handling, receipts, aliases, audit evidence,
closed outcomes, and exact replay behavior do not change.

Keyring coverage stays with the approval executor because approval startup must
prove that configured keys cover every live approval and apply receipt before
it accepts a mutation. This is metadata read authority exposed only through the
coverage function, not direct receipt-table access.

Migration 019 normalizes approval and coverage function metadata to the exact
readiness contract, adds the three topology functions, validates the 13 directly
referenced ordinary non-RLS relations and their common owner, removes all
non-owner grants from the five protected functions, and transfers the topology
functions to that owner.

Restricted-role execution exposed a transitive database boundary that direct
approval-function inspection does not show. The six tables mutated by approval
carry 19 user-defined triggers; a concrete approval INSERT or UPDATE executes
the applicable subset. The shared trigger graph can read
`automation_ruleset_activations`, `automation_ruleset_versions`, and
`runtime_deployments`, and can call one shared digest helper.

Migration 020 therefore validates an exact trigger semantic manifest. Each
entry binds the trigger name, relation, function, row/statement level, event,
timing, enabled state, constraint and parent relation identity, constraint
trigger status, deferrability, initially-deferred state, normalized `WHEN`
predicate, update-column vector, argument count and bytes, and old/new
transition-table bindings. An extra, missing, disabled, repointed, narrowed, or
re-timed trigger is contract drift. The migration expands the owner and non-RLS
relation boundary to 16 tables, replaces the two approval-table bindings to the
globally shared immutable-row trigger function with a dedicated approval-only
function, converts the resulting 18 unique trigger functions to internal
security-definer capabilities fixed to `search_path=pg_catalog`, normalizes the
shared digest helper, and removes every public and named non-owner function
grant from those 18 functions and the helper. Existing Apply functions and the
legacy global immutable-row function are not part of that revoke. It does not
grant application roles direct execution of the internal functions. Before
making those changes, it requires the other 17 trigger functions in the
resulting manifest to have the common owner; the new dedicated function is
created and transferred atomically.

The digest helper is also called by the existing Apply lock core and
finalization functions. Migration 020 requires the Apply lock wrapper, lock
core, and finalization functions to match the reviewed common owner, remain
security definers, have ordinary-function kind, and use exactly
`search_path=pg_catalog` before it changes helper metadata; it fails instead of
repairing drift. This prerequisite does not function-scope or certify the Apply
executor.

Both migrations deliberately preserve legacy relation ACLs so an upgrade
cannot silently break the still-direct reader or apply code.

## Readiness contract

Approval readiness has four layers:

1. catalog verification of exact function signature, result, language,
   volatility, strictness, parallel safety, row estimate, security-definer
   flag, fixed search path, owner, and ACL;
2. protected relation shape, common ownership, RLS state, and global table and
   column ACL verification;
3. direct-login role verification plus an exact executable manifest covering
   every `public.starring_*` routine and every public security-definer routine
   visible to the caller;
4. bounded rollback-only probes for topology, keyring coverage, and an invalid
   approval request that must deterministically return `invalid_input` without
   mutation.

Catalog and execution probes use transaction-local statement, lock, and
idle-in-transaction timeouts. Readiness classifies invalid contracts, missing
capabilities, excessive capabilities, incomplete key coverage, invalid results,
and database failures separately. It fails closed on unknown outcomes.

Approval component readiness also compares the three topology projections. It
does not claim that reader and apply capabilities are complete. Whole-service
ingress additionally requires their future component readiness and the final
global schema, object, executable, and relation-ACL sealing gate. The current
approval executable check does not inspect routines outside `public`, ordinary
non-`starring_*` security-invoker helpers, or objects outside its protected
relation list.

## Component completion and remaining slices

### Decision reader

Migration 021 replaced the direct multi-relation query with one versioned
security-definer projection. The reader role executes only its topology and read
functions. The accepted details and rollout contract are in
`2026-07-19-product-decision-reader-capability-design.md`.

### Apply executor

Migration 022 moved lock, bounded target-artifact projection, finalization, and
Apply-only keyring coverage behind an exact five-function manifest. Ruleset
parsing and deterministic preparation remain in Rust inside the same bounded
serializable transaction. The Apply role cannot execute approval, reader, or
internal helper functions. The accepted details and stopped-maintenance rollout
contract are in `2026-07-19-product-apply-executor-capability-design.md`.

### Rejection and status

Implement rejection persistence before provisioning its pool or role. Any
remaining deployment-status and promotion/publication routes that still read or
write protected relations must receive their own narrowly named capabilities.

### Final sealing

After all request-serving adapters are function-scoped, apply one atomic
migration that removes every non-owner table, view, sequence, and column grant
from the final protected object manifest. Then require every component readiness
check and a process-level executable manifest across every caller-accessible
non-system schema to pass together. A component-green but aggregate-red
deployment never opens ingress.

## Rollout and rollback

1. Deploy the three-pool Rust shape while legacy tests may use explicit cloned
   pools.
2. Apply migrations 019 through 022 in order under the reviewed owner handoff.
3. Reapply only the documented reader, approval, and Apply function manifests
   to the three direct-login roles.
4. Run reader, approval, Apply, and aggregate product-decision readiness. Any
   transitional relation ACL is an expected hard failure, not a warning.
5. Deploy and verify the three slices independently before composition.
6. Drain old connections, revoke legacy relation grants, apply the sealing
   migration, and run all aggregate gates before ingress.

Rollback never broadens grants. Roll back application traffic to the previous
version only after restoring its reviewed credential manifest. Do not retain a
shared privileged login as an emergency fallback. Every migration that replaces
a protected function removes prior grants, so its exact production grant must
be restored and readiness rerun before reopening traffic.

## Acceptance criteria

- Public construction requires three named pools and each operation uses only
  its assigned pool.
- Approval and keyring coverage work through a dedicated direct-login role.
- Exact approval replay preserves existing semantics.
- Direct access to the 16 protected relations, schema/database mutation,
  unrelated public `starring_*` and security-definer calls, grant option, and
  role membership are denied and detected.
- Missing capability, excessive capability, metadata drift, owner drift, RLS
  drift, mixed logical databases, and repeated role wiring fail closed.
- Hostile default and named function grants are removed by migration.
- Invalid prerequisites abort the migration atomically with no topology or ACL
  residue.
- Legacy relation grants are not silently removed before their callers are
  replaced, and their presence blocks production ingress.
- Exact trigger semantics include constraint, deferral, `WHEN`, update-column,
  argument, parent/constraint relation, and transition-table bindings.
- Final production ingress additionally rejects unexpected relations, views,
  sequences, routines, and schema privileges outside this component manifest.
- Package tests, real PostgreSQL tests, dependency guards, workspace tests,
  clippy with warnings denied, formatting, source-comment checks, and migration
  checks are green.
