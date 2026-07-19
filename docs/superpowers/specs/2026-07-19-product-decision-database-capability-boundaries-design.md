# Product decision database capability boundaries

Date: 2026-07-19

## Objective

Separate product-decision reads, approval mutations, and apply mutations into
independent PostgreSQL capabilities without changing the application-domain
ports, approval semantics, apply semantics, idempotency, audit evidence, or
deployment safety boundary.

The completed boundary must be suitable for production ingress. Compromise of
one direct-login database credential must not grant another operation family,
direct relation access, role escalation, schema mutation, or execution of an
unlisted `public.starring_*` function.

## Current state and staged target

`PostgresProductDecisions` currently routes queries, approvals, apply work, and
keyring coverage through one `PgPool`. Queries and parts of apply still issue
direct relation SQL. Approval is already one atomic security-definer function,
but the Rust adapter cannot give it a dedicated credential.

The Rust store moves immediately to three required pools:

| Pool | Final responsibility | Transitional state |
| --- | --- | --- |
| Decision reader | Approval preview and product-status projection | Direct SQL until its read function is introduced |
| Approval executor | Approval mutation and approval/apply receipt keyring coverage | Fully function-scoped in the first slice |
| Apply executor | Apply lock, artifact read, deterministic preparation, and atomic finalization | Existing SQL/functions until its boundary is completed |

The constructor intentionally has no single-pool production convenience. Tests
that exercise legacy semantics may explicitly provide three clones. Production
composition must provide three independently configured pools and direct-login
roles. A rejection executor is added only when the product rejection adapter
exists; an unused privileged credential is not provisioned in advance.

This work is staged to avoid an upgrade outage. The first slice does not revoke
legacy relation grants needed by the reader and apply adapters. Their presence
keeps the production ingress gate red until those adapters are function-scoped
and a later sealing migration removes every non-owner relation grant.

## Threat model

The boundary protects against:

- accidental pool wiring across environments or restored database clones;
- theft of one database credential;
- a caller receiving direct table or column privileges;
- `PUBLIC`, named-role, inherited-role, or grant-option privilege drift;
- execution of an unrelated `public.starring_*` function;
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

Every protected function is `SECURITY DEFINER`, `VOLATILE`, `STRICT`,
`PARALLEL UNSAFE`, fixed to `search_path=pg_catalog`, closed to `PUBLIC`, owned
by the common non-login relation owner, and assigned an explicit singleton row
estimate when set-returning. Migrations remove every named non-owner function
grant before ownership normalization. Production grants are always reapplied
after migration and checked before ingress.

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

The first migration normalizes approval and coverage function metadata to the
exact readiness contract, adds the three topology functions, validates the 13
protected ordinary non-RLS relations and their common owner, removes all
non-owner grants from the five protected functions, and transfers the topology
functions to that owner. It deliberately preserves legacy relation ACLs so an
upgrade cannot silently break the still-direct reader or apply code.

## Readiness contract

Approval readiness has four layers:

1. catalog verification of exact function signature, result, language,
   volatility, strictness, parallel safety, row estimate, security-definer
   flag, fixed search path, owner, and ACL;
2. protected relation shape, common ownership, RLS state, and global table and
   column ACL verification;
3. direct-login role verification plus an exact executable manifest for every
   `public.starring_*` function visible to the caller;
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
global manifest and relation-ACL sealing gate.

## Remaining slices

### Decision reader

Replace the direct multi-relation query with one versioned security-definer
function returning the exact validated projection required by Rust. Preserve
not-found and scope-hiding behavior. The reader role receives only its topology
and read function. Add rollback-only valid and impossible-scope probes and the
same exact executable manifest check.

### Apply executor

Move every apply lock, target artifact read, and finalization operation behind
an explicit apply manifest. Keep deterministic ruleset parsing and preparation
in Rust. Preserve serializable retry limits, commit-indeterminate handling,
artifact integrity, supersession, runtime request creation, receipts, and audit
evidence. The apply role must not execute approval or decision-read functions.

### Rejection and status

Implement rejection persistence before provisioning its pool or role. Any
remaining deployment-status and promotion/publication routes that still read or
write protected relations must receive their own narrowly named capabilities.

### Final sealing

After all request-serving adapters are function-scoped, apply one atomic
migration that removes every non-owner table and column grant from all protected
relations. Then require every component readiness check and a process-level
exact function manifest to pass together. A component-green but aggregate-red
deployment never opens ingress.

## Rollout and rollback

1. Deploy the three-pool Rust shape while legacy tests may use explicit cloned
   pools.
2. Apply the approval-boundary migration under the migrator credential.
3. Reapply only the documented topology, approval, and coverage grants to the
   three direct-login roles.
4. Run approval readiness. Any relation ACL needed by transitional reader or
   apply roles is an expected hard failure, not a warning.
5. Deploy and verify reader and apply slices independently.
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
- Direct relation access, schema/database mutation, unrelated function calls,
  grant option, and role membership are denied and detected.
- Missing capability, excessive capability, metadata drift, owner drift, RLS
  drift, mixed logical databases, and repeated role wiring fail closed.
- Hostile default and named function grants are removed by migration.
- Invalid prerequisites abort the migration atomically with no topology or ACL
  residue.
- Legacy relation grants are not silently removed before their callers are
  replaced, and their presence blocks production ingress.
- Package tests, real PostgreSQL tests, dependency guards, workspace tests,
  clippy with warnings denied, formatting, source-comment checks, and migration
  checks are green.
