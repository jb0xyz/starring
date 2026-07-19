# Product deployment-status capability

Date: 2026-07-20

## Objective

Replace the product deployment-status adapter's use of a mutation-capable
`PostgresRuntimeConvergence` credential with one independently least-privileged
PostgreSQL read capability. Preserve the existing product status semantics,
Rust persistence validation, exact Live proof, stable public failure codes,
fresh Discord authority window, and redacted backend errors.

This slice removes broad runtime relation access from the product API status
path. It does not scope runtime worker mutations, complete promotion
persistence, create production roles, or make the service eligible for public
ingress.

## Security boundary

The product deployment-status reader is a dedicated direct-login role and
pool. It is distinct from the decision reader, approval executor, Apply
executor, identity roles, promotion executor, and runtime worker.

The role executes exactly:

- `starring_product_deployment_status_reader_database_identity_v1()`;
- `starring_product_deployment_status_read_v1(TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, BYTEA)`.

The read inputs are deployment ID, server-derived promotion ID,
server-derived desired-target digest, tenant ID, installation ID, Discord
guild ID, authenticated principal ID, acting Discord user ID, and the
irreversible 32-byte product-session digest. The function never accepts a raw
product session, CSRF secret, OAuth credential, runtime credential, or
client-selected deployment result.

The database function returns zero, one, or at most two rows to Rust. Zero rows
uniformly cover malformed input, missing deployment, wrong tenant,
installation, principal, acting Discord user, session digest, disabled
principal, revoked session, expired session, or an unbound OAuth session. Rust
uses `LIMIT 2`, accepts exactly one row, maps zero rows to `NotFound`, and maps
two rows to an indeterminate persistence failure.

Promotion ID, desired-target digest, and guild remain server-derived
invariants from the authorized decision and installation projections. The
function first selects only by the active actor/session and exact tenant,
installation, and deployment identity. When the deployment exists but any of
those three server-derived invariants differs, it returns exactly one
`request_mismatch` row with every sensitive evidence field null. Rust maps that
closed outcome to `Indeterminate`. Missing deployment remains zero rows and
`NotFound`, preserving the established distinction without exposing the
persisted row on a mismatch.

Fresh Discord authority remains an in-process typed capability. PostgreSQL
cannot prove that Discord produced the evidence. The adapter binds the durable
principal, acting Discord user, session, installation, Discord application,
guild, current installation-authority revision and payload digest, and
deployment, then validates the evidence lifetime against the database
observation time.
A stolen status credential combined with a valid product-session digest is
outside this component boundary and remains covered by process credential
isolation and final ingress readiness.

## Function contract

`starring_product_deployment_status_read_v1` is a SQL, `VOLATILE`, `STRICT`,
`PARALLEL UNSAFE`, `SECURITY DEFINER` set-returning function with
`search_path=pg_catalog` and `ROWS 1`. It is owned by the common restricted
`NOLOGIN` owner of every protected relation. `PUBLIC`, named non-owner, and
grant-option execution are absent.

The result is:

```text
TABLE(
  request_outcome text,
  deployment_projection jsonb,
  activation_projection jsonb,
  promotion_projection jsonb,
  tenant_lifecycle_state text,
  installation_projection jsonb,
  historical_authority_projection jsonb,
  current_authority_projection jsonb,
  active_target_version bigint,
  artifact_projection jsonb,
  attestation_projection jsonb,
  serving_projection jsonb,
  database_now timestamp with time zone
)
```

Every evidence object has `evidence_format_version=1` and one exact `row`
object built with an explicit `jsonb_build_object` field manifest. The function
never uses `to_jsonb(table_row)`, so adding a database column cannot silently
expand the capability result.

The deployment row contains exactly the current `DEPLOYMENT_COLUMNS` evidence.
Activation, promotion, installation, historical authority, and current
authority contain only the fields needed to reproduce the current authority
decision and bind the fresh Discord evidence to the same installation
authority snapshot. The artifact row contains schema version, bounded definition,
content hash, and canonical content hash. The attestation row contains exactly
the current `ATTESTATION_COLUMNS` evidence. The serving row contains exactly
the current `SERVING_LEASE_COLUMNS` evidence. Rust evidence envelopes reject
unknown fields and unsupported format versions.

An exact request returns the raw bounded authority graph instead of an SQL
authority verdict. SQL therefore does not become a second authority engine.
Artifact evidence is returned only when the exact target row exists.
Attestation evidence is returned only for a Live deployment with a referenced
attestation. Serving evidence is returned only when the lane row exists. A
missing optional row remains distinguishable from malformed evidence without
exposing a SQL error as a public product result.

Raw evidence is an internal integrity input. It is never placed in an outward
error, `Debug` representation, HTTP response, metric label, or log field.

## Snapshot and concurrency semantics

The capability body is one SQL statement composed from materialized request
clock, exact actor/session/deployment selection, request-match gate, authority
graph, artifact, attestation, and serving CTEs. It does not call
`starring_runtime_lock_current_authority`, does not use `FOR SHARE`, and does
not take an application row lock.

The adapter opens a new `READ COMMITTED, READ ONLY` transaction and sets
transaction-local statement, lock, idle-in-transaction, search-path, and
identifier-quoting settings. The capability call is the only data-reading
statement and is therefore its linearization point. `READ COMMITTED` is
intentional: configuration statements cannot pin an older repeatable-read
snapshot before the actual status observation. One SQL statement observes one
coherent PostgreSQL snapshot while avoiding contention with runtime heartbeat,
disconnect, recovery, and authority mutations.

`database_now` is the statement timestamp for the same observation. Rust
requires the Discord evidence observation to precede it and the evidence
expiry to follow it. Read authority is bounded to 30 seconds and Apply replay
authority to 5 seconds, preserving the current contract.

Concurrent authority, active-pointer, deployment, attestation, or serving
changes may produce the coherent state immediately before or after the commit.
They must never produce a mixed false-Live projection.

## Authority projection

SQL selects the same durable authority facts used by the runtime convergence
store without locking them. Rust reduces that exact evidence to the closed
outcomes:

- `not_evaluated` for cancelled or superseded deployments;
- `exact`;
- `scope_mismatch`;
- `binding_mismatch`;
- `active_mismatch`;
- `lifecycle_inactive`.

The Rust comparison covers the linked and Applied product activation, exact
activation-pending promotion journal, tenant and installation lifecycle,
historical installation authority, current installation authority, resource
binding identity, active RuleSet pointer, target version, and target content
hash. Unknown outcomes fail closed.

Terminal deployment semantics retain priority over later authority changes.
For nonterminal deployments, one shared Rust projector maps the closed
authority outcome through the existing status policy before it considers
runtime pending or Live evidence. SQL never returns an authoritative
availability or authority outcome.

## Rust integrity boundary

`automation-runtime-convergence-postgres` owns persistence interpretation. A
new versioned evidence module deserializes the explicit envelopes and reuses:

- `DeploymentRow::decode` and `RuntimeDeployment::restore`;
- scalar-to-snapshot projection comparison;
- desired-target digest recomputation;
- activation, promotion, tenant, installation, historical authority, current
  authority, active pointer, and artifact comparison;
- RuleSet schema, structural, canonical hash, and content hash validation;
- attestation record parsing and digest recomputation;
- exact snapshot-to-attestation comparison;
- serving epoch and revision validation;
- exact tenant, installation, deployment, attestation, process, generation,
  target, connection, serving, heartbeat, and expiry comparison.

The existing runtime-worker `status` path and the product evidence path feed
one shared pure status projector. Availability, reason code, retryability, and
Live are never authoritative SQL outputs. This prevents PL/pgSQL from becoming
a second runtime domain engine and prevents future product/runtime status
drift.

The product adapter is split by responsibility under `deployment_status/`:

- `config` owns bounded timeouts;
- `contract` owns exact database identities and result shapes;
- `query` owns the read-only transaction and capability call;
- `row` owns product actor/session/scope evidence validation;
- `projection` owns stable public failure-code mapping;
- `readiness` owns capability, relation, topology, trigger, and helper checks;
- `mod` owns the public adapter and port orchestration.

`PostgresProductDeploymentStatuses::new` accepts the dedicated status-reader
`PgPool`. It never accepts `PostgresRuntimeConvergence` or any runtime mutation
credential. Tests may keep a separate privileged runtime adapter only to
advance fixture state.

## Relation and persistence manifest

The capability directly or transitively reads 12 data relations plus the
topology relation:

1. `product_control_plane_identity`;
2. `product_principals`;
3. `product_auth_sessions`;
4. `runtime_deployments`;
5. `activation_requests`;
6. `authoring_promotions`;
7. `product_tenants`;
8. `automation_installations`;
9. `automation_installation_authority_versions`;
10. `automation_ruleset_activations`;
11. `automation_ruleset_versions`;
12. `runtime_attestations`;
13. `runtime_serving_leases`.

Readiness requires all 13 to be ordinary non-RLS tables with one common owner.
The status role has no table or column privilege on any of them.

The persisted evidence contract additionally requires the exact enabled user
trigger manifest on the three runtime relations:

- four deployment triggers for projection, artifact, policy shadow, and delete
  protection;
- two attestation triggers for projection and immutability;
- two serving-lease triggers for transition and delete protection.

The RuleSet artifact support contract requires the canonical-hash helper and
validated content-integrity constraint. Migration checksums, the common
restricted owner, exact trigger/function metadata, and audited DDL access are
the function-body integrity boundary. Readiness does not pretend to provide a
cryptographic function-body attestation.

## Readiness

Component readiness verifies:

- exact external function identity, named arguments, result, language,
  volatility, strictness, parallel mode, security mode, search path, row
  estimate, owner, and ACL;
- exact global executable allowlist across every public `starring_*` and public
  security-definer routine for the status role;
- trusted `public` schema;
- all 13 relation types, owners, RLS states, global table and column ACLs;
- direct-login role properties, database/schema privileges, role membership,
  owner membership, temporary-object capability, and grant option;
- exact runtime trigger and supporting helper metadata;
- logical database identity, database name, and direct session role;
- one canonical impossible tuple returning zero rows.

The impossible probe proves execution and non-enumerating behavior without
creating production data. Positive restricted-role tests provide the
functional projection proof.

## Migration behavior

Migration `202607200001` preflights the complete relation, owner, schema,
persistence-support, and existing topology contract before creating either
status function. It rejects function-name collisions and incompatible
metadata. It creates no new authority helper and changes no existing helper or
relation ACL.

The migration creates the topology and status functions, transfers them to the
common owner, strips `PUBLIC`, hostile default, named non-owner, and grant-option
execution, and verifies the final catalog contract. It restores transaction
session settings. Any invalid prerequisite or postcondition aborts atomically
without function, ownership, ACL, or setting residue.

The migration does not create login roles, embed credentials, grant relation
access, or silently revoke transitional grants required by other adapters.
Production role bootstrap grants the two exact functions after migration and
must pass readiness before the pool is admitted.

## Performance and product constraints

The request path uses one capability round trip, one database snapshot, no row
locks, no automatic retries, and no unbounded query. Every selected identity is
indexed and the result cardinality is at most one under the reviewed schema.

Persistence evidence has hard existing bounds: deployment snapshot 256 KiB,
artifact definition 512 KiB, attestation record 256 KiB, and small scalar
serving evidence. The worst-case internal result is therefore bounded but too
large to treat as free. Before claiming a commercial status SLO, production-
shaped evaluation must record pool acquisition and end-to-end p50, p95, p99,
payload bytes, timeouts, and saturation. Metric labels exclude tenant,
principal, guild, promotion, deployment, session, and digest values.

If measurement shows polling bandwidth or JSON validation is material, the
next reviewed version may add an immutable artifact-validation certificate or
bounded cache keyed by content hash. V1 does not trade away persistence
integrity for an unmeasured optimization.

## Acceptance matrix

- A restricted status role returns Pending for an exact Requested deployment.
- Exact Live requires the matching immutable attestation and a connected,
  serving, unexpired exact lease.
- Missing, disconnected, expired, or identity-mismatched serving evidence is
  Pending and never false Live.
- Retryable and blocked failures expose only the existing stable public code;
  private controller code, message, identity, and fencing evidence never cross
  the product boundary.
- Cancelled, superseded, active-target drift, binding drift, and inactive
  product authority preserve their existing outcomes.
- Wrong tenant, installation, deployment, principal, acting Discord
  user, session digest, malformed input, disabled principal, revoked session,
  and expired session return zero rows uniformly.
- Wrong server-derived promotion, desired-target digest, or guild returns one
  payload-free `request_mismatch` outcome and remains indeterminate.
- Discord application, current installation-authority revision, or current
  authority payload-digest drift is indeterminate and never produces Live.
- Unknown evidence format, extra JSON field, malformed persisted row, unknown
  authority outcome, invalid artifact, invalid attestation digest, or malformed
  serving projection fails closed.
- Concurrent heartbeat, disconnect, recovery, and authority changes produce
  one coherent old or new snapshot without blocking the writer.
- Direct `SELECT`, DML, DDL, truncate, temporary objects, schema creation,
  unrelated protected functions, role membership, and grant option are denied.
- Missing or extra execution, relation or column privilege, owner drift, RLS
  drift, schema drift, function drift, trigger drift, helper drift, logical
  database mismatch, or repeated role fails readiness.
- Hostile default privileges are removed and invalid migration prerequisites
  roll back without residue.
- Existing deployment-status, Apply replay, runtime convergence, workspace,
  Clippy, formatting, dependency, package, comment, and PostgreSQL gates remain
  green.

## Rollout and remaining work

Rollout order is migrate as the common owner, grant only the two status
functions to a new direct-login role, build the dedicated pool, pass component
readiness, switch product status composition, verify restricted-role Pending
and Live, then remove the product API's old runtime-store credential.

This slice does not authorize a hot rollout against an independently
credentialed runtime worker whose internal helper grants were removed by the
Apply migration. Runtime rollout continues to require the documented
drain-migrate-grant-readiness-restart sequence.

After this slice, promotion journal/publication/activation-link persistence,
rejection, production approval environment, snapshot cryptography, closed HTTP
facade, runtime worker capabilities, declarative role/default-privilege
bootstrap, global process executable sealing, and release evidence remain
required before ingress.
