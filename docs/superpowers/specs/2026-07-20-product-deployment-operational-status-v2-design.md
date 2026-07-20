# Product deployment operational status V2 design

Date: 2026-07-20

Status: implementation contract

## Outcome

Expose a bounded, authenticated product view of runtime convergence without
weakening the existing deployment-status V1 contract or disclosing controller
authority. The view distinguishes normal progress, retry timing, blocked
operator work, product-authority drift, and exact serving freshness.

This increment is an additive observation capability. It does not recover a
deployment, mutate Discord, activate a RuleSet, grant database privileges, or
make the HTTP library a runnable production service.

## Public contract

The authenticated route is:

```text
GET /v2/installations/{installation_id}/promotions/{promotion_id}/deployment
```

The response contains only:

- the exact requested installation and promotion identity
- the product decision observation time
- a closed product status
- a closed runtime convergence phase
- the current convergence-attempt number
- the last failed attempt number when present
- a stable public failure code and retryability
- a retry deadline and whether it is waiting or due
- one closed operator action when intervention is required
- an attested deployment revision and attempt when present
- one closed serving-freshness state and safe lease times when applicable

It never contains a runtime failure identifier, backend failure message,
controller identifier, fencing token, process identifier, attestation
identifier, runtime build revision, shard identifier, panel report digest,
database error, Discord response, SQL text, or credential material.

The public phase and serving values are closed enums. Unknown or inconsistent
database evidence becomes an internal error rather than a new string value.

## Authority and identity

The browser supplies only the opaque product session plus installation and
promotion path selectors. The application authenticates the session, derives
the actor, obtains fresh Discord read authority, loads the product decision, and
derives the exact deployment selector from that server-owned decision. A client
cannot select a deployment ID, target digest, attempt number, guild, tenant,
authority revision, or serving identity.

Invalid authentication, session, actor, tenant, installation, and Discord scope
produce no database evidence row. A valid request whose exact deployment tuple
does not match receives only `request_mismatch`; every evidence column,
including all attempt scalars, is null. The Rust adapter treats a payload-bearing
mismatch as corruption.

## Database observation contract

The existing V1 function signature, result columns, grants, and JSON evidence
format remain unchanged. V2 adds three scalar columns outside the V1 evidence
envelopes:

- `deployment_convergence_attempt_no`
- `deployment_last_failure_attempt_no`
- `attestation_convergence_attempt_no`

The scalar separation prevents a V2 field from silently expanding a
`deny_unknown_fields` V1 evidence envelope.

One owner-only `starring_product_deployment_status_read_core_v2` function reads
the original evidence and all three attempt values in one SQL query. V1 and V2
are projection wrappers over that core. This yields one coherent core
observation for the deployment-status evidence capability. Authentication,
fresh Discord authority, and product-decision observation remain separate
application steps and are not described as one database snapshot.

All three functions are `SECURITY DEFINER`, `VOLATILE`, `STRICT`,
`PARALLEL UNSAFE`, fixed to `search_path=pg_catalog`, owned by the common
non-login relation owner, and bounded to one result row. The core has owner-only
execution. Migration files create no environment role and grant no production
capability.

## Least-privilege rollout

V2 uses a separate direct-login reader role and pool. That role receives only:

- database `CONNECT`
- `public` schema `USAGE`
- the dedicated operational-status V2 database-identity function
- `starring_product_deployment_status_read_v2`

It receives no table, sequence, schema-create, temporary-table, V1 status,
owner-core, mutation, retention, Apply, approval, promotion, runtime, or grant
option capability. V1 keeps its existing role and identity-function grant so
old and new application revisions can overlap during a rolling deployment.
Aggregate readiness remains red if the V2 login has a missing or excess
executable capability, any user relation or sequence access, unapproved user
schema capability, topology mismatch, function contract drift, trigger drift,
or runtime-attempt schema drift.

## Runtime invariants

Attempt zero is valid only before the first claim. Every failure attempt is
positive and no greater than the current attempt. An attestation is present only
for the current positive attempt and exact deployment revision.

Retry state is derived from database observation time:

- `retry_waiting` requires `observed_at < retry_not_before`
- `retry_due` requires `observed_at >= retry_not_before`

A blocked runtime failure exposes only `recover_blocked_deployment`. Product
authority failure exposes only `restore_product_authority`. A failure class and
operator action may not be mixed.

`live` requires all of the following to agree:

- the durable runtime phase is Live
- the product decision points to the exact deployment and target digest
- the attestation revision is exact
- the attestation attempt equals the current convergence attempt
- the serving lease belongs to the exact attested process and generation
- the lease is connected
- `last_heartbeat_at <= observed_at < lease_expires_at`

A Live phase with missing attestation, missing lease, identity mismatch,
disconnect, or expiry remains product Pending and reports the corresponding
safe serving-freshness enum. Lease timestamps are exposed only for the exact
identity states `disconnected`, `expired`, and `fresh`; mismatch evidence is
redacted.

## Compatibility and failure behavior

V1 callers keep byte-compatible response DTOs and the original route. The V1
SQL signature, result shape, grants, and evidence envelopes remain unchanged.
The internal runtime projector now classifies an exact-identity mismatch before
disconnect or expiry, so mismatched lease timestamps cannot influence its
reason code; the V1 HTTP response still projects that condition as Pending. The
V2 trait and router are separate so a V1 facade cannot accidentally expose V2.
Response validation runs after facade projection and rejects impossible field
combinations before serialization.

Malformed numeric values, overflow, duplicate attempt fields, partial
attestation evidence, mismatched attempts, impossible phase/failure/action
combinations, invalid clock ordering, multiple rows, and payload-bearing
request mismatches fail closed as bounded internal errors.

## Required evidence

The increment is complete only when automated tests prove:

- V1 function and HTTP compatibility
- request-mismatch scalar and JSON redaction
- non-enumerating invalid authentication and scope
- pristine attempt zero
- retry waiting and retry due at the exact database clock boundary
- blocked failure and operator recovery attempt progression
- product-authority drift classification
- Live attestation-attempt binding
- missing, mismatched, disconnected, expired, and fresh serving evidence
- malformed, negative, zero-only, and overflowing attempt rejection
- one-row and one-core-observation behavior under concurrent updates
- separate-role readiness and executable allowlist enforcement
- denial of relation access, DML, DDL, temporary objects, and unrelated
  functions
- V2 HTTP authentication, path validation, timeout/error mapping, closed
  response validation, and sensitive-field absence
- workspace tests, PostgreSQL suites, clippy with warnings denied, formatting,
  dependency guards, static secret scanning, and source-comment scanning

## Remaining production work

This section records the baseline at design acceptance. The production facade
and `tools/starring-api` composition root were subsequently implemented with
distinct database pools, aggregate readiness, loopback binding, and graceful
shutdown. `CURRENT_STATE.md` is authoritative for implementation status.
Runtime recovery remains a separate authenticated mutation capability and must
never accept a failure ID or attempt identity from this status response.
