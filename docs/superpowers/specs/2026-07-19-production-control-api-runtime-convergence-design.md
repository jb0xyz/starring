# Production control API and runtime convergence design

Date: 2026-07-19

Status: accepted implementation contract

The `Current architecture and gaps` section records the baseline at design
acceptance. It is not a live progress report. `CURRENT_STATE.md` is the
authoritative implementation inventory; this document remains authoritative
for the target contract and sequencing.

Supersedes: the unimplemented production-edge and runtime-convergence sections of
`2026-07-18-authoring-promotion-bridge-design.md`

## Outcome

Turn a server-owned `PreviewReady` authoring session into a production Discord
automation through an authenticated, tenant-scoped, auditable control plane:

```text
Discord identity
  -> opaque Starring session
  -> fresh guild authority
  -> atomic authorized authoring snapshot
  -> promotion and immutable RuleSet publication
  -> server-rendered approval preview
  -> distinct payload-bound approval
  -> guarded active-pointer apply
  -> durable runtime convergence
  -> exact Live attestation with a fresh serving lease
```

The model remains a design proposer. It cannot authenticate a user, select a
guild, publish, approve, apply, call Discord, write PostgreSQL, or declare a
deployment Live. Every authority-bearing value comes from authenticated product
state or a fresh server-side Discord observation.

This increment starts from a server-owned authoring session. It persists every
session generation required to reproduce a promotion, but it does not add a raw
snapshot import endpoint. A later chat transport may create and advance sessions
only through the same server-side harness writer. Codex account orchestration,
LLM billing, and public template distribution are outside this boundary.

## Product assumptions and defaults

- Starring initially runs on one Mac mini behind Cloudflare Tunnel.
- The HTTP listener binds only to `127.0.0.1`; the tunnel is the public ingress.
- Cloudflare Access may protect the origin, but it is not product identity or
  Discord guild authorization.
- PostgreSQL is the source of truth for product sessions, installations,
  authoring generations, promotion state, activation state, runtime convergence,
  idempotency, and audit evidence.
- One Starring Discord application and bot serve all initial installations.
- A tenant is a product-owned Discord guild installation, not an individual
  user. The schema retains separate `tenant_id` and `installation_id` fields so a
  future organization tenant can own multiple installations without rewriting
  existing identities.
- Initial provisioning binds a bot installation to a guild through an operator
  workflow. Public bot-install and organization-management APIs are deferred.
- Product OAuth requests only `identify`. Starring does not request or persist a
  user OAuth `guilds` token.
- Guild authority for a mutation is fetched with the bot credential immediately
  before that mutation. Owner, `ADMINISTRATOR`, or `MANAGE_GUILD` satisfies the
  initial manager policy.
- The default activation policy requires one approval from a manager other than
  the requester and expires after 24 hours. Policy revision, quorum, and TTL are
  installation-owned values, never request-body values.
- Approval and apply remain separate actions. The apply actor may be the
  requester or approver if they still have fresh manager authority; requester
  self-approval is always forbidden.
- No customer quota or business rate limit is introduced in this increment.
  Strict body limits, deadlines, bounded concurrency, and ingress-level abuse
  protection remain mandatory resource-safety controls.
- The first runtime implementation is a controlled drain and restart. Hot swap
  remains forbidden until top-level interaction routing is version-pinned.

Defaults are configuration, not constants embedded in domain logic:

| Setting | Initial default | Security purpose |
| --- | ---: | --- |
| OAuth flow lifetime | 10 minutes | Bounds login CSRF and code replay state |
| Product session absolute lifetime | 12 hours | Bounds stolen-session lifetime |
| Product session idle lifetime | 30 minutes | Retires abandoned admin sessions |
| Product action replay guarantee | 7 days | Bounds receipt retention and HMAC key retirement lag |
| Product receipt alias capacity | 32 per receipt | Bounds maintenance work across repeated key rotations |
| Maintenance purge batch | 1,000 parent rows | Bounds one database maintenance transaction |
| Fresh write authority age | 5 seconds | Limits Discord role-change race |
| Read authority cache age | 30 seconds | Protects tenant data without a Discord call storm |
| HTTP JSON body limit | 64 KiB | Bounds memory and parser work |
| Database statement deadline | 2 seconds | Bounds control-plane pileups |
| Discord HTTP deadline | 5 seconds | Fails closed on authority uncertainty |
| Privileged request deadline | 10 seconds | Gives callers a deterministic result |
| Runtime convergence deadline | 90 seconds | Bounds one controlled restart attempt |
| Gateway drain deadline | 15 seconds | Finishes accepted work before forced shutdown |

## Current architecture and gaps

The current repository already provides the following safety boundaries:

- `design-harness` exports an owned, non-deserializable
  `PreviewReadyArtifactV1` only after full validation and simulation evidence.
- `authoring-promotion` keeps a monotonic durable promotion journal and publishes
  an immutable RuleSet version without changing the active pointer.
- `authoring-promotion-postgres` persists that journal and resumes interrupted
  publication and activation-request creation.
- `automation-ruleset-activation` enforces self-approval prohibition, exact
  payload-bound approval, binding and active-baseline checks, readiness, leases,
  and guarded pointer mutation.
- PostgreSQL triggers bind product activation requests to the exact
  `ActivationPending` promotion journal.
- `automation-runtime` dispatches the hydrated RuleSet, while existing instances
  retain their pinned version.

The production gaps are material:

- There is no HTTP server, OAuth flow, product session store, CSRF boundary, or
  fresh Discord guild authorization adapter.
- Existing smoke CLIs accept `--actor`; those IDs are test input and are not an
  authentication mechanism.
- `VerifiedPrincipalV1::from_trusted_edge` is publicly constructible and the
  transport could bypass authentication if production composition used it
  directly.
- The application loads the owned artifact and promotion authority through two
  sequential ports. A session or authority change between reads can pair values
  that never existed in one durable generation.
- Authoring session snapshots are not yet durable PostgreSQL product state.
- Activation and approval tables are not consistently scoped by tenant and
  installation at every query boundary.
- PostgreSQL checks protect record shape, but the runtime database role is not
  yet denied direct DML. A writer with table access can set the current
  transaction-local approval-context GUC and attempt forbidden state changes.
- `ActivationService::apply` changes the active pointer and marks an activation
  request `Applied`, but that does not reload the running gateway.
- The gateway owns its top-level RuleSet by value for its process lifetime.
- Hydration and declared-panel reconciliation currently run only during the
  `interaction-smoke` startup path.
- Panel reconciliation reports transient or unresolved-channel skips as report
  entries rather than a hard convergence failure.
- A one-time gateway `Ready` event does not prove that the process remains
  connected and serving.

The design below closes those gaps without weakening the existing promotion,
activation, readiness, or instance-version boundaries.

## Trust boundaries

### Browser

The browser is untrusted. It may supply:

- a human message to a future authoring-turn endpoint
- an opaque session cookie
- a CSRF token returned by Starring
- an installation, session, promotion, or deployment identifier in a path
- expected generation or digest preconditions
- an idempotency key
- a bounded rejection reason

The browser may not supply or override:

- tenant or principal identity
- Discord user, guild, role, or permission claims
- authoring session snapshots, RuleSet JSON, or resource bindings
- installation RuleSet key
- approval policy, quorum, or TTL
- requester, approver, rejector, or apply actor
- activation target or active baseline
- apply attempt ID
- runtime state or Live attestation

Unknown JSON fields are rejected so a client cannot believe an ignored authority
field was honored.

### HTTP edge

`tools/starring-api` owns Axum routing, cookies, exact-origin checks, request
limits, response serialization, OAuth redirects, and dependency composition. It
does not import raw promotion or activation stores. Handlers call only the pure
product application facade.

The edge passes an opaque session credential to that facade. It never constructs
a verified principal. `Authorization`, `Cookie`, `Set-Cookie`, OAuth `code`,
OAuth tokens, CSRF tokens, and raw idempotency keys are marked sensitive and
excluded from logs and traces.

### Product application

`authoring-application` remains pure. It owns use-case sequencing and accepts
opaque credentials through an authentication port. Its verified-principal type
has no public unchecked constructor. The product facade is the only production
route to promotion, approval, rejection, apply, and status projection.

The facade derives the Discord actor from the authenticated session, obtains a
fresh guild-authority observation from a port, loads a single atomic durable
session-and-installation snapshot, and invokes the existing promotion and
activation services.

### PostgreSQL adapters

`authoring-application-postgres` owns product authentication, installation,
session-generation, idempotency, and audit persistence. It also implements the
single atomic authorized-snapshot read. SQLx and migrations do not enter pure
crates.

Existing PostgreSQL promotion, RuleSet, activation, instance, and panel stores
remain authoritative for their domains. Production mutations are exposed to the
API role through narrowly scoped procedures or adapter-owned transactions, not
general table DML.

### Discord adapters

The OAuth adapter handles only identity. The guild-authority adapter uses the bot
credential to fetch the target guild, member, and roles and computes permissions
server-side. It returns a short-lived observation bound to application, guild,
Discord user, permission bits, authority revision digest, and observation time.

An unavailable, incomplete, mismatched, or stale observation denies the
operation. Cached data cannot authorize a mutation.

### Runtime worker

`tools/starring-runtime` has a separate database role and bot credential. It can
claim convergence work, read the active target and exact artifacts, reconcile
panels, run the gateway, heartbeat a serving lease, and write an exact
attestation. It cannot create product sessions, approve activations, or change
the active pointer.

### Model

The LLM sees only design tools. No authentication, installation, promotion,
approval, apply, deployment, Discord, or PostgreSQL tool is added. A dependency
guard keeps these crates outside `design-harness`.

## Discord OAuth and product session

### Start

`GET /oauth/discord/start` performs these steps:

1. Validate an optional `return_to` against a fixed same-origin path allowlist.
2. Generate independent 32-byte random OAuth `state` and browser nonce values
   with the operating-system CSPRNG.
3. Store only SHA-256 digests of both values, the exact redirect URI, expiry,
   and normalized return path in `product_oauth_flows`.
4. Set `__Host-starring_oauth` to the raw browser nonce with `Secure`,
   `HttpOnly`, `SameSite=Lax`, `Path=/`, no `Domain`, and the flow lifetime.
5. Redirect to Discord's authorization endpoint with the exact registered
   redirect URI, `response_type=code`, `scope=identify`, and the raw state.

No Discord guild or permission claim is requested in this flow.

### Callback

`GET /oauth/discord/callback` performs these steps:

1. Require one `code`, one `state`, and the OAuth nonce cookie.
2. Hash state and nonce and atomically consume one unexpired, unconsumed matching
   flow before exchanging the code. A failed exchange requires a new login flow.
3. Exchange the code using an exact redirect URI, form encoding, the configured
   client ID, and a secret loaded only from the environment or Keychain.
4. Require the returned scope to be exactly sufficient for `identify`, fetch
   `/users/@me`, and reject bot or system identities.
5. Canonicalize the Discord user ID into a product principal. Display names and
   avatars are non-authoritative profile data.
6. Revoke every returned Discord OAuth credential before issuing a product
   session. Revocation failure is fail-closed for login. Tokens are never
   persisted and are zero-retention request memory.
7. Generate independent 32-byte product-session and CSRF secrets. Store only
   their SHA-256 digests with principal, creation time, last-seen time, absolute
   expiry, idle expiry, authentication time, and revocation state.
8. Set `__Host-starring_session` with `Secure`, `HttpOnly`, `SameSite=Lax`,
   `Path=/`, no `Domain`, and the shorter remaining lifetime. Clear the OAuth
   nonce cookie and redirect only to the stored allowlisted path.

OAuth errors never echo Discord response bodies, codes, or tokens to the client.

### Authenticated requests

- The raw session cookie is hashed before lookup and never enters structured
  logs, errors, traces, or metrics labels.
- Authentication rejects revoked, absolute-expired, or idle-expired sessions.
- `last_seen_at` is updated at a bounded interval rather than on every request.
- `GET /v1/me` returns only a display-safe principal view. The callback sets the
  raw CSRF secret in a separate Secure, non-HttpOnly, SameSite=Strict host cookie;
  the secret is never serialized into an identity response.
- Every state-changing endpoint requires the session cookie, exact configured
  `Origin`, exactly one CSRF cookie, and exactly one `X-CSRF-Token`. The edge
  constant-time compares the cookie and header before the application hashes
  the proof and constant-time compares it against the session row.
- CORS is disabled for unknown origins. If a separate first-party web origin is
  configured, it is an exact allowlist with credentials and no wildcard.
- `POST /v1/logout` atomically revokes the session, clears the cookie, and is an
  exact replay success.
- Session secrets rotate only by reauthentication in the first version. No
  bearer API token or long-lived refresh token is exposed to the browser.

### Fresh guild authority

For a protected installation, Starring fetches the guild and requesting member
through the bot API and computes effective guild permissions from the everyone
role, member roles, ownership, and administrator semantics. The observation is
accepted only when:

- application and guild match the installation
- the bot installation is active
- member identity matches the authenticated Discord principal
- every referenced role belongs to the fetched guild state
- the observation is within the operation's freshness bound
- the effective policy grants access

Promotion, approval, rejection, and apply always perform a new fetch. Sensitive
GETs may reuse a successful observation for at most 30 seconds. A role removal
therefore revokes mutation authority on the next request without waiting for the
product session to expire.

Discord authority and PostgreSQL cannot be read in one distributed transaction.
The accepted race is bounded by the five-second write observation. The
application binds its observation digest and timestamp into the audit event and
rechecks the installation identity inside the subsequent database transaction.
It never turns the observation into long-lived authorization.

## Durable product model

All external IDs use typed Rust wrappers and canonical database encodings.
Discord snowflakes are canonical decimal text so no signed-integer assumption
leaks into persistence. Secret digests are 32-byte values or lowercase
64-character hexadecimal values with database length checks.

### Identity and sessions

`product_principals`

- `principal_id` primary key
- unique canonical `discord_user_id`
- disabled flag and identity revision
- created, updated, and last-authenticated timestamps
- display profile separated from authority fields

`product_oauth_flows`

- state digest primary key and unique browser-nonce digest
- exact redirect URI and allowlisted return path
- created and expiry timestamps
- consumed timestamp and terminal result code
- immutable after consumption

`product_auth_sessions`

- session digest primary key and principal foreign key
- CSRF digest
- created, authenticated, last-seen, idle-expiry, and absolute-expiry timestamps
- revoked timestamp and reason
- no raw cookie, OAuth access token, or OAuth refresh token

### Tenant and installation authority

`product_tenants`

- opaque stable `tenant_id` primary key
- lifecycle state and display metadata
- no user ID as tenant identity

`automation_installations`

- stable `installation_id` primary key and tenant foreign key
- Discord application and guild IDs
- product-owned `RuleSetKey`
- lifecycle state and current authority revision
- unique application, guild, and RuleSet-key identity

`automation_installation_authority_versions`

- immutable `(installation_id, revision)` primary key
- canonical resource bindings and binding fingerprint
- policy revision, required approvals, and activation TTL
- authority payload digest and creation provenance
- no human membership cache

Changing bindings or policy appends a version and moves the installation head.
An existing promotion remains bound to its original authority version and fails
fresh apply preconditions when current bindings or policy no longer match.

### Immutable authoring generations

`authoring_sessions`

- session ID, tenant, installation, and owner principal
- current generation and lifecycle state
- created and updated timestamps
- owner, tenant, and installation immutable after creation

`authoring_session_generations`

- immutable `(session_id, generation)` primary key
- harness snapshot schema version and an authenticated-encryption envelope
- ciphertext, nonce, key identifier, cipher-suite version, and authenticated
  metadata digest stored in separate bounded columns
- original resource-binding catalog and fingerprint
- installation authority revision
- bounded summary, stage, candidate revision, and candidate hash projections
- creation timestamp, writer request digest, and harness contract revision

The server-side harness writer validates durable transcript limits and snapshot
invariants before inserting a generation. Advancing a session inserts one
immutable generation and compare-and-swaps the head in one transaction. Exact
writer replay reuses the generation; a different replay body conflicts.

Authoring transcripts can contain user-supplied secrets. The writer encrypts the
validated snapshot before persistence with a key loaded from Keychain or a
production secret reference. PostgreSQL never receives the plaintext snapshot
as a durable column, and the envelope binds tenant, installation, session,
generation, schema version, and binding fingerprint as authenticated metadata.
Key rotation appends a new envelope generation through an owner-only maintenance
path; API and runtime roles cannot request bulk decryption.

There is no endpoint that accepts snapshot JSON, a `PreviewReadyArtifactV1`, a
RuleSet definition, bindings, candidate hashes, or stage flags. Only the harness
process can produce generation rows.

### Product decisions, idempotency, and audit

`product_action_receipts`

- tenant, installation, principal, endpoint-domain, and idempotency-key digest
  form the unique scope
- request digest binds every semantic input and expected precondition
- target resource, resulting revision/state, HTTP disposition class, and
  completion timestamp
- exact replays within the retained replay window return the recorded semantic result
- a reused key with a different request digest returns `409`

Raw idempotency keys are never stored. Promotion continues using its existing
domain-separated request digest. Apply derives `ApplyAttemptId` deterministically
from the product endpoint domain, immutable promotion identity, authenticated
actor, and idempotency scope. The database verifies the promotion's one-to-one
activation link before mutation, so an HTTP retry cannot create a second apply
attempt without requiring an untrusted pre-lock activation identifier.

The authority resource-context fingerprint and approval binding fingerprint are
different identities. The resource-context v2 fingerprint covers the complete
binding map and binds authority versions, authoring evidence, runtime targets,
and audits. The approval binding v1 fingerprint covers guild, binding revision,
and only the required binding subset. Approval contexts carry the latter; code
never compares it directly with the full authority fingerprint.

`product_audit_events`

- append-only event ID, tenant, installation, principal, product-session ID
  digest, action, target, request ID, and idempotency receipt
- authority-observation digest, effective permission bits, and observation time
- expected and actual generation, payload digest, binding fingerprint, policy
  revision, active baseline, and resulting state where relevant
- stable result code, dependency latency classes, and timestamp
- no cookies, tokens, raw idempotency keys, human message bodies, or RuleSet JSON

Decision mutation and its idempotency receipt and audit event commit in one
transaction. An audit failure fails the decision; there is no successful
unaudited privileged action.

`product_action_receipt_audit_evidence` permanently preserves the receipt's
scope, endpoint, request digest, target, semantic result, completion time, and
replay-policy version without retaining an idempotency digest, HMAC key ID, or
key-material fingerprint. The immutable audit event references this evidence.
After the seven-day replay guarantee, a maintenance-only procedure may delete
the live receipt and its aliases while the audit and forensic receipt evidence
remain append-only. Purge eligibility is `now >= replay_guaranteed_until`; a
delayed purge may extend replay opportunistically but never shortens the
guaranteed window.

The maintenance procedure processes only `product_approve_v1` receipts in this
increment. It locks at most 1,000 parents with `SKIP LOCKED`, deletes at most 32
aliases per parent before the parent receipt, and reports whether eligible work
remains. A separate read-only keyring coverage probe must succeed before a
retired HMAC key is removed. The safe rotation order is new-plus-old writers,
old-writer drain, one full replay window, purge to an empty backlog, coverage
probe with the proposed keyring, new-only writers, then old-secret destruction.

### Tenant-scoping additions to existing tables

Product-authored activation requests gain non-null `tenant_id` and
`installation_id` shadow columns. Constraints require them to match the linked
promotion and product approval context. Legacy/manual rows retain an explicit
legacy scope until migrated or retired.

Every product query includes tenant, installation, and resource identity even
when the resource ID is a cryptographic digest. An inaccessible cross-tenant ID
returns the same `404` as a missing ID.

## Atomic authorized session snapshot

The two sequential `OwnedSessionArtifactPort` and `PromotionAuthorityPort`
reads are replaced in the production path by one
`AuthorizedPromotionSnapshotPort` operation. Conceptually:

```text
load_authorized_preview_ready(
  authenticated principal,
  installation ID,
  session ID,
  expected generation,
  fresh Discord authority observation
) -> AuthorizedPromotionSnapshotV1
```

One repeatable database transaction:

1. Selects tenant, installation head, session head, exact immutable generation,
   owner, and exact installation-authority version.
2. Requires the authenticated principal to own the session.
3. Requires session tenant and installation to match the path and fresh Discord
   observation.
4. Requires the session head to equal `expected_generation`.
5. Requires the immutable generation's authority revision and bindings to agree
   with the selected installation authority under the promotion policy.
6. Copies the exact snapshot, original bindings, installation identity, RuleSet
   key, policy, requester Discord user, and authority identities into one owned
   result.

After the transaction, the pure application restores the snapshot through
`DesignSession::restore_intent_recipe` with the original bindings and exports a
fresh typed `PreviewReadyArtifactV1`. Full harness validation remains mandatory.
Later session edits cannot change the copied generation, and a concurrent head
change makes the original expected generation stale for a new request.

The port returns no partial artifact or partial authority. Generation mismatch,
ownership mismatch, installation mismatch, non-PreviewReady state, corrupt
snapshot, and stale authority are distinct internal errors but are mapped to a
non-enumerating external error contract.

## Product application facade

The pure application exposes credential-based use cases rather than public
unchecked principal construction:

- authenticate and project current user
- promote owned PreviewReady session
- load promotion status and server-rendered approval preview
- approve the exact bound payload
- reject the exact bound payload with a reason
- apply an approved activation request
- load deployment and Live status
- logout or revoke the current product session

The transport cannot reach `PromotionService`, `ActivationRequestStore`,
`ActivationService::approve`, `approve_bound`, or PostgreSQL stores directly.
The application facade adds an approval method that always reaches the existing
payload-bound store path. The unbound legacy approval method remains available
to existing domain tests and manual tooling but is absent from production
composition.

The facade orders a privileged action as follows:

1. Authenticate opaque credential and verify CSRF and origin evidence for a
   mutation.
2. Load the path-scoped installation without disclosing another tenant.
3. Fetch fresh Discord authority for the authenticated principal.
4. Begin or resume the operation's idempotency receipt.
5. Load exact server state and enforce expected generation or digest.
6. Call the existing promotion or activation safety boundary.
7. Commit receipt and audit event with the decision.
8. Return a transport-neutral product projection.

## REST contract

All JSON request types use `deny_unknown_fields`. Responses use stable
snake-case enums, RFC 3339 UTC timestamps, `Cache-Control: no-store` for identity
and approval data, and an ingress or generated `X-Request-Id`.

### Identity

```text
GET  /oauth/discord/start
GET  /oauth/discord/callback
GET  /v1/me
POST /v1/logout
```

### Promotion and decisions

```text
POST /v1/installations/{installation_id}/authoring/sessions/{session_id}/promotions
GET  /v1/installations/{installation_id}/promotions/{promotion_id}
GET  /v1/installations/{installation_id}/promotions/{promotion_id}/approval-preview
POST /v1/installations/{installation_id}/promotions/{promotion_id}/approvals
POST /v1/installations/{installation_id}/promotions/{promotion_id}/rejections
POST /v1/installations/{installation_id}/promotions/{promotion_id}/apply
GET  /v1/installations/{installation_id}/promotions/{promotion_id}/deployment
```

Promotion request:

```json
{
  "expected_generation": 12
}
```

The endpoint requires `Idempotency-Key`. A newly created promotion returns
`201`; an exact replay returns `200` with `replayed: true`. The result identifies
the promotion, published immutable target, current product state, approval
preview digest, and activation-request expiry. It never returns raw RuleSet or
snapshot JSON.

Approval preview is rendered only from persisted server state and contains the
structural preview, exact guild and automation identity, immutable target,
resource-binding summary, active baseline, policy, expiry, and canonical payload
digest. It includes an `ETag` derived from that digest.

Approval request:

```json
{
  "expected_payload_digest": "<canonical lowercase digest>"
}
```

Rejection request:

```json
{
  "expected_payload_digest": "<canonical lowercase digest>",
  "reason": "The channel policy needs revision."
}
```

Apply request:

```json
{
  "expected_payload_digest": "<canonical lowercase digest>"
}
```

All three mutations require `Idempotency-Key`, the product session, exact
origin, CSRF token, and a fresh manager observation. Rejection reasons are
trimmed, normalized, and limited to 1,000 Unicode scalar values. Actor fields,
guild fields, policy fields, target fields, and runtime fields are rejected as
unknown.

Apply returns `202 Accepted` with `runtime_pending` once the active pointer is
known to target the exact artifact and a durable convergence record exists. It
returns `200` only for an exact replay whose current runtime projection is
already Live. Pointer mutation alone never produces `live`.

Deployment status includes:

- promotion and activation state
- exact target version and content hash
- runtime convergence state and phase
- attempt count and safe stable failure code
- retry scheduling or operator-action requirement
- current attestation identity and last serving heartbeat when Live

It excludes internal exception text, Discord bodies, SQL errors, leases, and
credentials.

### Health

```text
GET /health/live
GET /health/ready
```

Liveness checks only that the process event loop is responsive. Readiness checks
configuration validity, migration compatibility, database access, and required
worker coordination. Discord outages do not kill liveness; they make protected
operations fail closed and may make runtime readiness degraded.

### Error envelope

```json
{
  "error": {
    "code": "stale_generation",
    "message": "The authoring session changed. Reload and try again.",
    "request_id": "<opaque request id>",
    "retryable": false
  }
}
```

External status mapping:

| Status | Meaning |
| ---: | --- |
| 400 | malformed path, header, or JSON |
| 401 | absent, invalid, expired, or revoked product session |
| 403 | authenticated principal lacks fresh guild authority or CSRF/origin proof |
| 404 | missing or inaccessible installation, session, promotion, or deployment |
| 409 | stale generation/digest, state conflict, or idempotency-key conflict |
| 422 | server-owned candidate no longer validates or bounded input is invalid |
| 429 | local concurrency budget exhausted, with `Retry-After` |
| 502 | Discord or OAuth upstream returned an invalid response |
| 503 | database, Discord authority, or runtime coordination unavailable |
| 504 | a bounded dependency deadline expired |

Database constraint names, SQL text, Discord bodies, and internal IDs not
already present in a successful resource view never enter errors.

## Approval and apply invariants

- Promotion requires the session owner and fresh manager authority.
- Publication always creates an inactive immutable RuleSet version.
- The approval payload is rendered from the exact promotion and activation
  records; client prose cannot alter it.
- The approver must be a different Discord user from the requester.
- Every approval stores the exact expected payload digest and authenticated
  Discord user derived from the product session.
- Duplicate exact approval is an idempotent replay. A different digest or actor
  under the same key conflicts.
- Approval replay reauthorizes the caller against the current authority head but
  verifies the recorded result against its immutable historical authority,
  audit, and receipt evidence. Authority rotation cannot rewrite or suppress a
  valid retained result.
- Rejection is terminal for that activation request and requires a new promotion
  idempotency key after redesign. Rejection never mutates the active pointer.
- Apply requires linked product authority, satisfied quorum, unexpired request,
  exact approval payload, fresh manager authority, unchanged active baseline,
  unchanged resource-binding fingerprint, current server policy, readiness, and
  a valid activation lease.
- Binding, active-baseline, or policy drift supersedes the request before pointer
  mutation. A missing, oversized, or hash-mismatched immutable target is
  persistence corruption rather than drift; it returns the bounded invalid-candidate
  result without hiding the forensic signal behind supersession.
- An indeterminate product-apply commit is resolved only by replaying the same
  idempotency key. The committed outcome contains pointer, `Applied`, Requested
  deployment, receipt, and audit together; the rolled-back outcome contains
  none of them. A new key is never used to guess the result.
- Apply replay reauthorizes the caller against the current installation-authority
  head while verifying the retained receipt, audit, deployment, and decision
  against their immutable historical authority. An old authority observation
  cannot replay a result after authority rotation.
- A guild and RuleSet key may have only one unresolved runtime deployment. A new
  apply cannot advance the pointer while the current target is runtime-pending,
  unless an explicit operator recovery first resolves or supersedes it.
- Existing instance interactions remain pinned to the instance's stored RuleSet
  version throughout top-level runtime restart.

## Runtime convergence

### Durable deployment model

`runtime_deployments`

- deployment ID and exact activation-request and promotion IDs
- tenant, installation, guild, RuleSet key, version, and content hash
- binding fingerprint and policy revision
- immutable desired-target digest
- durable state, phase, revision CAS, attempt number, and next retry time
- lease owner, lease epoch, lease expiry, and last stable error code
- created, updated, Live, superseded, and blocked timestamps

`runtime_attestations`

- immutable deployment and attempt identity
- exact guild, RuleSet key, version, content hash, and binding fingerprint
- runtime build revision and process-instance ID
- strict panel-reconciliation generation and report digest
- gateway shard identity and Ready timestamp
- attestation digest and creation timestamp

`runtime_serving_leases`

- one row per guild and RuleSet key
- exact deployment, attestation, process instance, and lease epoch
- connected/serving state, heartbeat time, and lease expiry
- revision CAS and no customer-controlled fields

The desired-target digest remains version 1 for this increment. It already
binds the immutable installation-authority revision, whose referenced row binds
policy revision, quorum, TTL, and resource bindings. `policy_revision` remains
an immutable deployment shadow for audit and query parity, while
`desired_target_digest_version` explicitly records version 1. A future digest
version uses dual-read and versioned golden vectors; version 1 is never changed
in place.

Product apply uses one serializable transaction rather than composing the
independently committing activation and runtime stores. A lock phase copies the
exact server-owned target, baseline, binding, policy, current serving identity,
and next runtime generation. Rust builds the existing version-1 Requested
snapshot and digest from that locked projection. A finalizer in the same
transaction revalidates every locked identity, performs the baseline CAS,
transitions `Approved -> Applying -> Applied`, inserts the Requested deployment,
and appends the receipt, aliases, audit, and forensic receipt evidence.

A deferred invariant requires each product activation transitioning to `Applied`
to have exactly one matching canonical deployment whose target is the active
pointer in that transaction. Historical `Applied` activations retain their exact
immutable deployment after a newer target supersedes the pointer. Migration
preflight fails on an existing ambiguous `Applied` row; startup never guesses a
deployment or previous runtime to backfill it.

### State machine

```text
Pending
  -> Converging
  -> Live

Converging
  -> RetryWait
  -> Superseded
  -> Blocked

RetryWait
  -> Converging

Live
  -> Pending       when its serving lease expires or process identity changes
  -> Superseded    when a newer guarded target is accepted
```

`Converging` has a durable phase:

```text
claim -> drain -> hydrate -> reconcile_panels -> start_gateway -> attest
```

Every transition uses revision CAS and a lease epoch. A worker may write phase,
failure, attestation, or heartbeat only while it owns the current unexpired
epoch. An expired worker cannot overwrite a newer worker's evidence.

Runtime authorization preserves two identities. The deployment retains the exact
historical installation-authority revision used by Apply for audit and digest
verification. Every claim, mutation, recovery, heartbeat, and status read also
checks the current authority head for active lifecycle and exact binding revision,
fingerprint, and resource-binding map equality. A policy, quorum, or TTL-only
rotation therefore keeps the same deployment eligible and Live; an actual binding
change or a spoofed fingerprint with different binding content fails closed.

Retryable dependency failures use bounded exponential backoff with jitter and a
maximum delay. Deterministic corruption, target mismatch, or unsupported schema
moves to `Blocked` with a stable code and requires operator action. If the active
pointer no longer matches the immutable target, the deployment is `Superseded`,
not retried.

### Controlled restart

For one claimed target, the runtime worker:

1. Locks or leases the guild and RuleSet-key convergence lane.
2. Verifies the current active pointer exactly matches the deployment target.
3. Stops reading new top-level gateway events.
4. Allows the currently accepted interaction to finish within the drain
   deadline, then closes the old shard.
5. Hydrates the exact active artifact and repeats the existing readiness gate.
6. Resolves the exact binding revision and verifies its fingerprint.
7. Reconciles every declared panel while no top-level dispatcher is accepting
   events.
8. Converts the panel report into a strict certificate. Any transient skip,
   unresolved channel, missing message, ambiguous result, or partial action is a
   convergence failure rather than success.
9. Starts the gateway with the exact immutable RuleSet and bindings.
10. Waits for gateway `Ready` and a local serving signal under the convergence
    deadline.
11. In one database transaction, locks the active pointer and deployment,
    rechecks the exact target, inserts the immutable attestation, and acquires the
    serving lease.
12. Marks the deployment `Live` only after that transaction commits.

`automation-runtime` gains a cancellation-aware `run_until_shutdown` contract
and explicit readiness/connection signals. Event processing remains serial in
the first production version, which makes bounded drain behavior deterministic.

### Meaning of Live

The product projects `Live` only when all of these are true at read time:

- activation request is `Applied`
- current active pointer exactly matches the promotion target
- deployment is `Live`
- immutable attestation matches deployment target and binding fingerprint
- attested process instance owns the current serving lease
- serving lease is unexpired and its latest heartbeat reports connected/serving
- strict panel certificate exists for the attested reconciliation generation

A stale database `Live` state after a process crash therefore projects
`runtime_pending` or `unavailable`, not Live. The runtime heartbeats only while
the shard reports a serving connection. Disconnect or heartbeat expiry removes
Live immediately and schedules recovery.

Gateway `Ready` is necessary but insufficient: it proves one successful
connection event, while the serving lease proves the attested process is still
the current connected owner.

## PostgreSQL security model

Production uses separate credentials:

| Role | Capability |
| --- | --- |
| `starring_owner` | `NOLOGIN`; owns tables, types, triggers, and functions |
| `starring_migrator` | deployment-only DDL and ownership handoff |
| `starring_api` | execute product auth, session, decision, and scoped-read functions only |
| `starring_runtime` | claim convergence, read exact active artifacts, attest, and heartbeat only |
| `starring_maintenance` | execute bounded identity and receipt retention procedures only |
| `starring_observer` | sanitized operational views only |

Deployment revokes `PUBLIC` privileges, revokes direct DML on authority-bearing
tables from API and runtime roles, and sets restrictive default privileges for
future objects. Credentials and pool sizes are separate per process.

Security-definer functions:

- schema-qualify every object
- set a fixed safe `search_path`
- validate canonical IDs and expected revisions
- lock the exact tenant-scoped rows
- perform one allowed transition only
- write the idempotency receipt and audit event in the same transaction
- return a minimal typed projection

The existing product-approval trigger and transaction-local context digest stay
as defense in depth, but the API role cannot directly insert an approval or set
up a forged state transition. The guarded procedure sets any required internal
transaction context itself after validating the exact parent request.

Tenant-bearing tables enable and force row-level security as a second defense
against accidental unscoped reads. Policies are driven by a transaction context
set only inside scoped adapter transactions. RLS is not treated as protection
from a fully compromised application process; least-privilege procedures and
separate runtime/API credentials are the stronger mutation boundary.

Immutable authoring generations, authority versions, audit events, and runtime
attestations reject update and delete through triggers owned by
`starring_owner`. Retention is implemented by an explicit archival procedure,
not ad hoc deletes from the production role.

The API and runtime roles cannot execute retention procedures. The maintenance
role cannot call product decision functions or directly select, insert, update,
or delete authority-bearing tables. Each maintenance adapter sets bounded
transaction-local statement and lock timeouts before invoking a security-definer
procedure.

Production startup fails readiness when the database migration version, grants,
function signatures, or role capability probe do not match the binary's expected
contract.

## HTTP and secret hardening

- TLS terminates at Cloudflare; the local service accepts only loopback traffic.
- Host and forwarded-origin values are accepted only from the configured tunnel
  path. Untrusted forwarded headers are ignored.
- Request IDs are validated for a bounded safe character set or regenerated.
- Sensitive request and response headers are marked before tracing middleware.
- JSON content type is required; duplicate or oversized critical headers are
  rejected.
- Per-route timeouts, a global in-flight semaphore, database-pool bounds, and
  graceful shutdown prevent resource exhaustion.
- Security responses include `Content-Security-Policy`, `frame-ancestors 'none'`,
  `X-Content-Type-Options: nosniff`, a strict referrer policy, and no-store where
  appropriate. HSTS is configured at the public TLS edge.
- OAuth client secret, bot token, database DSNs, Cloudflare credentials, and
  digest peppers are loaded from environment or Keychain references. They never
  appear in source, committed configuration, database rows, panic output, or
  status endpoints.
- Panic handling emits a request ID and stable internal-error code. Release
  binaries do not return backtraces.
- Graceful shutdown first removes API readiness, drains bounded HTTP work, then
  releases pools. Runtime shutdown stops serving heartbeats before closing its
  shard so status cannot remain falsely Live.

## Threat model

| Threat | Required mitigation and evidence |
| --- | --- |
| OAuth login CSRF or callback replay | Random state plus browser nonce, digest-only storage, exact redirect URI, atomic one-time consume, short expiry |
| Authorization-code or OAuth-token leak | Sensitive-field redaction, no persistence, exact callback, immediate fail-closed revocation |
| Product session theft | CSPRNG opaque cookie, digest-only storage, Secure/HttpOnly/Lax host cookie, idle and absolute expiry, revocation |
| Cross-site mutation | Exact Origin, per-session CSRF secret, SameSite cookie, no wildcard credentialed CORS |
| Principal spoofing | No actor IDs in body, no public unchecked principal constructor, credential authenticated inside application facade |
| Cross-tenant IDOR | Tenant and installation in every query, RLS, scoped procedures, indistinguishable 404 |
| Stale Discord role | Fresh bot-side guild/member/role fetch for every mutation, bounded observation, fail closed |
| Session generation TOCTOU | Immutable generations, head CAS, one atomic artifact-and-authority snapshot |
| Client RuleSet or binding injection | No raw snapshot/artifact/RuleSet/binding endpoint; typed export after harness restoration and validation |
| Self-approval or quorum bypass | Existing domain invariant, payload-bound approval, fresh distinct Discord identity, DB guarded procedure |
| Approval of stale preview | Expected canonical digest, immutable payload context, baseline/binding/policy checks at apply |
| Retry causes duplicate action | Required idempotency key, scoped request digest, deterministic apply attempt, atomic receipt |
| Direct SQL state forgery | Separate roles, revoked table DML, narrow security-definer transitions, existing constraints and triggers |
| Runtime split brain | Per-target lease epoch, revision CAS, expired-worker fencing, one serving lease |
| Pointer reported as Live | Separate deployment, exact attestation, connected serving heartbeat, strict status predicate |
| Partial panel installation | Strict certificate rejects transient, unresolved, ambiguous, or partial report entries |
| New panels handled by old rules | Stop old dispatcher before reconciliation; start exact new dispatcher afterward; no hot swap |
| Runtime dies after attestation | Short serving lease and connection-gated heartbeat; expiry removes Live |
| Dependency outage | Deadlines, fail-closed authority, retryable convergence journal, readiness degradation, no unsafe replay |
| Log or metric cardinality leak | Redacted secrets and bodies; opaque request IDs; no user-supplied strings as metric labels |

Host root compromise, PostgreSQL owner compromise, Discord platform compromise,
and volumetric attacks beyond the ingress capacity are outside the application's
cryptographic trust model. Backups, host patching, Cloudflare controls, and
credential rotation remain required operational defenses.

## Failure recovery

| Failure point | Durable state | Safe recovery |
| --- | --- | --- |
| OAuth state mismatch or expiry | no session | start a new OAuth flow |
| OAuth exchange or revocation failure | consumed flow, no session | start a new flow after dependency recovery |
| Product session expiry | revoked or expired session | reauthenticate |
| Discord authority unavailable | no decision mutation | retry after Discord recovers |
| Session head changed | no promotion mutation | reload and submit current generation with a new key when semantics changed |
| Artifact restoration invalid | no publication | repair authoring session through the harness |
| Crash after promotion journal creation | `Prepared` | existing promotion resume path |
| Crash after immutable publication | `Prepared` or `Published` | exact publication reuse and journal resume |
| Crash after activation request | `Published` or `ActivationPending` | exact request link and resume |
| Approval or rejection response lost | committed receipt and decision | exact idempotency replay |
| Apply readiness failure | `Approved` | retry same request after environment repair with a new apply key |
| Baseline, binding, or policy drift | `Superseded` | new preview and promotion |
| Product apply commit indeterminate | unknown until replay | retry the same idempotency key only |
| Legacy `Applied` row has no exact deployment | invalid migration state | migration fails; operator repairs or explicitly retires the legacy record before retrying |
| Runtime process crashes during convergence | leased `Converging` | lease expiry, fenced reclaim, resume from safe phase |
| Runtime process crashes while Live | stale serving lease | status loses Live; worker reconverges and writes a new process attestation |
| Panel reconciliation partial | `RetryWait`, old dispatcher stopped | retry exact target; never attest partial state |
| Gateway Ready timeout | `RetryWait` | close shard and retry with backoff |
| Active pointer changes during attestation | `Superseded` | never write Live for stale target |
| Deterministic corrupt target | `Blocked` | operator diagnosis; no automatic mutation |

Recovery operations are idempotent, target the same immutable identities, and
never infer success from absence of an error response.

## Observability and operational evidence

Structured logs carry request ID, operation domain, safe resource digest,
tenant-safe internal identifier, state transition, stable result code, duration,
and retry disposition. They do not carry secrets, OAuth fields, cookies, human
messages, rejection text, or RuleSet definitions.

Metrics include:

- HTTP request count, latency, in-flight count, timeout, and stable error code
- OAuth starts, callback outcomes, replay rejection, and revocation failure
- product session creation, expiry, revocation, and auth failure class
- Discord authority latency and fail-closed reason class
- promotion, approval, rejection, apply, and idempotent replay outcomes
- stale generation, payload, binding, baseline, and policy conflicts
- activation recovery and indeterminate-apply counts
- deployment queue age, phase duration, retry count, blocked count, and Live lag
- panel strict-certificate failures by stable code
- gateway connected state, serving lease age, heartbeat failures, and Live count

Metric labels use bounded enums and component names, never raw guild, user,
session, promotion, or error strings. Audit records provide identity-specific
forensics under controlled access.

Initial release evidence reports p50, p95, and p99 for control actions and each
runtime phase. Performance claims are made only after repeated local and
disposable-guild measurements. The control-plane SLO program treats
`pointer_applied` and `live_attested` as separate timestamps.

## Modular implementation plan

### Pure application

Keep `crates/authoring-application` and split it by cohesive responsibility:

```text
src/
  lib.rs
  authentication.rs
  authorized_snapshot.rs
  promotion.rs
  decisions.rs
  status.rs
  error.rs
```

It retains no Axum, SQLx, Twilight, or OAuth HTTP dependency. Existing public
wire-independent types remain compatible where safe; the unchecked verified
principal constructor is removed from production reach.

### Product PostgreSQL adapter

Add `crates/authoring-application-postgres` for:

- OAuth-flow and product-session stores
- principals, tenants, installations, and immutable authority versions
- immutable authoring session generations
- the atomic authorized-promotion snapshot
- scoped idempotency receipts and audit events
- product status projections and activation facade adapters

This crate may depend on SQLx and existing PostgreSQL adapters but does not own
HTTP or Discord clients.

### HTTP edge

Add `tools/starring-api` as a thin Axum binary:

- route and strict DTO modules
- OAuth and Discord HTTP adapters
- cookie, CSRF, origin, limits, timeout, tracing, and security-header middleware
- environment/Keychain configuration validation
- dependency composition and graceful shutdown

An architecture test fails if the binary imports raw promotion or activation
stores instead of the product application facade.

### Runtime convergence

Add a pure `crates/automation-runtime-convergence` state machine and orchestration
ports, plus `crates/automation-runtime-convergence-postgres` for durable claims,
leases, attestations, backfill, and status projections.

Extend `automation-runtime` only with cancellation, drain, and connection/ready
signals. Extend panel installation with a strict certificate adapter without
changing the existing report semantics used by smoke tools.

Add `tools/starring-runtime` as the production worker. Keep
`tools/interaction-smoke` as test and manual tooling; it is not a deployable
production control surface.

## Phased implementation and commit plan

Each phase is a separate reviewable commit and leaves its scoped tests green.

1. **Design contract**
   - Land this specification and architecture assertions.
2. **Atomic application boundary**
   - Replace sequential promotion reads with one authorized snapshot port.
   - Hide unchecked principal construction.
   - Add payload-bound approve, reject, apply, preview, and status facades.
3. **Durable product identity and authoring state**
   - Add tenants, installations, immutable authority versions, principals,
     OAuth flows, product sessions, immutable authoring generations,
     idempotency, and audit migrations and adapters.
   - Add tenant and installation shadows to product activation rows.
4. **Database least privilege**
   - Add guarded transition functions, RLS, immutable-row triggers, grants,
     default privileges, bounded archival procedures, keyring readiness, and
     role capability tests.
5. **Discord authentication and authorization**
   - Implement identify-only OAuth, opaque sessions, CSRF, exact origin, token
     revocation, and fresh guild-manager authority adapter.
6. **Production control HTTP API**
   - Add strict REST DTOs, error mapping, idempotency behavior, timeouts,
     redaction, security headers, health, and graceful shutdown.
7. **Runtime convergence domain and persistence**
   - Add deployment/outbox, lease-epoch state machine, backfill, attestation,
     serving lease, and product status projection.
8. **Controlled runtime restart**
   - Add gateway drain/cancellation/connection signals, strict panel
     certificate, production worker, retry recovery, and fencing.
9. **Production evidence and operations**
   - Add launchd service definitions without secrets, migration/runbook,
     backup/restore drill, dashboards, and disposable-guild evidence.

No phase combines model tools with deployment authority. No commit adds secrets,
customer identifiers, or gateway URLs. Workspace tests, PostgreSQL tests,
clippy with warnings denied, formatting, dependency guards, and static secret
checks remain green at every merge boundary.

## Acceptance tests

### Pure application

- A transport-like caller cannot construct a verified principal or authority
  context from raw IDs.
- Promotion calls the atomic snapshot port once and cannot pair values from
  different generations.
- Wrong owner, tenant, installation, generation, authority revision, bindings,
  or non-PreviewReady state cannot submit a promotion.
- Restored exact snapshot exports the same typed candidate identities.
- Product approval always calls the payload-bound path; unbound approval is not
  reachable through the facade.
- Actor, guild, target, policy, and apply-attempt identities are server-derived.
- Product state never maps pointer `Applied` directly to Live.

### PostgreSQL

- OAuth state and nonce are one-time, expiry-bound, digest-only, and race-safe.
- Product session lookup, idle/absolute expiry, revocation, and CSRF comparison
  behave correctly under concurrent requests.
- Immutable session generations advance by head CAS and exact replay only.
- Atomic snapshot reads one matching generation and authority version under
  concurrent session and installation updates.
- Every product query rejects cross-tenant and cross-installation access.
- Product activation shadow identities match promotion and approval context.
- Mutation, idempotency receipt, and audit event commit or roll back together.
- API and runtime roles cannot directly insert, update, or delete authority
  tables and cannot execute each other's functions.
- RLS and immutable-row triggers reject deliberately unscoped or mutating SQL.
- Existing promotion-link and approval-context trigger tests remain green.

### OAuth and HTTP

- Start emits only `identify`, exact redirect URI, fresh state, and the required
  host-cookie attributes.
- Callback rejects missing, duplicate, mismatched, replayed, consumed, and
  expired state or nonce.
- OAuth tokens and codes are absent from logs, errors, database, and snapshots.
- Revocation failure issues no product session.
- Mutations reject missing cookie, invalid/expired session, missing or duplicate
  CSRF cookie/header, cookie/header mismatch, stale backend CSRF proof,
  missing/wrong Origin, wildcard origin, oversized body, wrong content type,
  duplicate critical header, and unknown JSON field.
- Cross-tenant known IDs and random IDs produce indistinguishable 404 responses.
- Stable internal errors contain no SQL or Discord response details.
- Exact idempotent replay inside the seven-day guarantee returns the same
  semantic result; mismatched reuse is `409`.
- Receipt purge preserves byte-stable audit and forensic evidence, deletes no
  more than the configured batch and alias capacity, and cannot be executed by
  API or runtime roles.
- A retired HMAC key cannot be removed until the live-receipt coverage probe
  succeeds for the proposed keyring and purge reports no eligible backlog.
- Concurrency and dependency timeout exhaustion fail boundedly without process
  starvation.

### Discord authority and product decisions

- Owner, administrator, and manager are accepted; ordinary or removed member is
  denied.
- A cached read observation never authorizes a write.
- Guild, application, member, or role mismatch fails closed.
- Requester self-approval remains rejected through HTTP and direct adapter tests.
- Payload digest mismatch, expiry, rejection, withdrawal, supersession, binding
  drift, policy drift, and active-baseline drift preserve the old pointer.
- A role revoked after approval prevents apply until a currently authorized
  manager performs it.
- Apply exact replay uses the same deterministic attempt identity.

### Runtime convergence

- A product activation transitioning to Applied atomically creates exactly one deployment; no startup backfill infers runtime identity.
- Two workers racing to claim produce one current lease epoch; the expired loser
  cannot write phase, attestation, or heartbeat.
- Drain stops new events and finishes one accepted serial interaction within the
  deadline.
- Hydrating a different version, content hash, or binding fingerprint never
  reaches panel reconciliation or Live.
- Every transient, unresolved, missing, ambiguous, or partial panel outcome
  rejects the strict certificate.
- New panels are reconciled only while the old top-level dispatcher is stopped.
- Gateway Ready timeout and disconnect close the shard, remove Live, and enter a
  retryable state.
- Attestation transaction rejects an active pointer changed concurrently.
- Runtime crash in every durable phase is reclaimed safely after lease expiry.
- A stale process heartbeat cannot retain Live after a new process takes over.
- Existing pinned instance interactions continue using their stored version.

### End-to-end and operational

- A server-generated PreviewReady generation promotes, publishes inactive,
  renders a digest, receives a distinct manager approval, applies, converges,
  and reports Live only with exact attestation and serving heartbeat.
- Repeating each network request after an injected lost response produces no
  duplicate publication, approval, pointer mutation, deployment, or audit gap.
- Killing API, PostgreSQL connection, runtime worker, and gateway at every
  transition yields only documented recoverable states.
- Backup restore preserves immutable session generations, promotion and
  activation identities, idempotency, audit, deployment, and attestation links.
- Ordinary CI uses deterministic fakes and PostgreSQL integration tests. Live
  OAuth and disposable-guild checks are explicit release evidence and never run
  against a customer guild.
- Production launch configurations bind API to loopback, use distinct database
  roles, contain no secrets, and fail readiness on schema or grant drift.

## Release gate

The vertical slice is production-eligible only when:

- every acceptance category above is automated or explicitly classified as a
  disposable-guild release check
- the complete workspace, PostgreSQL suites, clippy, formatting, Promptfoo
  static checks, dependency guards, and secret scan are green
- a security review confirms no transport route reaches raw promotion,
  activation, Discord mutation, or database stores
- a recovery drill proves pointer-applied/runtime-pending converges after process
  restart without a second pointer mutation
- a serving-lease expiry drill proves a dead runtime stops reporting Live
- API and runtime database role probes demonstrate least privilege
- operational documentation covers migration, rollback, backup, restore,
  credential rotation, audit access, blocked deployment recovery, and emergency
  runtime shutdown

Until those gates pass, the service may be exercised as a staging control plane
but must not claim production Live automation.
