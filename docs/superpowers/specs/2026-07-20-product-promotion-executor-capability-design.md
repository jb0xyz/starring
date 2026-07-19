# Product promotion executor capability

Date: 2026-07-20

Status: implementation contract

## Objective

Make authenticated promotion submission executable with a dedicated PostgreSQL
credential that can do only the exact work required to turn one current,
server-owned `PreviewReady` authoring generation into one inactive immutable
RuleSet and one linked product activation request.

The capability preserves deterministic promotion and activation identity,
canonical RuleSet hashing, inactive publication, exact activation-journal
linking, bounded recovery, and the existing approval payload. It removes direct
relation SQL from production promotion composition and closes the authorized
snapshot read-to-journal-write TOCTOU interval.

This slice does not add public ingress. It creates the safe production adapter
that a later closed HTTP facade can compose.

## Current gap

`AuthoringApplication::promote_owned_session` authenticates the opaque product
credential with CSRF, obtains fresh Discord Promote authority, and atomically
loads an encrypted authoring generation with its installation authority.

The verified result is then reduced to `StartPromotionV1`. The product request
ID, authenticated session fingerprint, raw product idempotency secret, and fresh
authority evidence do not reach `PromotionSubmissionPort`. A generic
`PromotionService` uses three PostgreSQL stores that issue direct relation SQL.

That leaves five production blockers:

1. the promotion login needs broad relation privileges;
2. database mutations are not bound to the authenticated product session and
   fresh Promote observation;
3. the authoring head and authority can change between snapshot read and the
   first durable journal write;
4. exact replay currently requires loading the old authoring head before the
   product adapter can discover an already accepted request;
5. partial promotion state has no durable authenticated admission evidence and
   completed promotion has no product receipt or audit evidence.

The domain saga remains the semantic reference, but the generic service is not
the production execution mechanism for this capability.

## Security boundary

The browser does not provide tenant, principal, Discord actor, guild, RuleSet,
binding, policy, target, publication, activation, receipt, or audit identity.

The pure application is the only constructor of authorized promotion access and
submission wrappers. It combines:

- product request ID;
- authenticated actor and current product-session fingerprint;
- server-resolved installation scope;
- fresh Discord Promote evidence;
- product idempotency secret;
- requested authoring session and generation;
- for first admission only, the typed `StartPromotionV1` built from the atomic
  server snapshot.

The PostgreSQL adapter accepts only these wrappers. The generic
`PromotionSubmissionPort`, `PromotionService`, and generic stores remain for
domain tests and non-product tooling. There is no blanket implementation from a
generic port to the authorized product port.

The production adapter owns a distinct direct-login PostgreSQL pool. Its login
has no relation or column privileges and only the exact function grants in this
contract.

The model receives no new tool or authority. The HTTP crate continues to depend
only on a facade and cannot import promotion, RuleSet, activation, or PostgreSQL
stores.

## Pure application contract

### Product idempotency secret

Replace the promotion command's direct `authoring_promotion::IdempotencyKey`
field with `ProductPromotionIdempotencyKeyV1` in `authoring-application`.

`ProductPromotionIdempotencyKeyV1`:

- accepts the same one-to-128-byte canonical product key alphabet;
- owns `Zeroizing<String>`;
- implements redacted `Debug`;
- is not `Serialize` or `Deserialize`;
- has no public general-purpose `as_str` or `expose_secret` method;
- is consumed by the application when constructing authorized access and
  submission wrappers.

The application parses a transient domain `IdempotencyKey` from this secret when
it builds `StartPromotionV1`. The domain key's crate-private accessor remains
crate-private. It is not widened for the PostgreSQL adapter.

`AuthorizedPromotionAccessV1` and `AuthorizedPromotionSubmissionV1` expose the
raw bytes only through one deliberately named adapter boundary:

```rust
pub fn with_product_idempotency_secret<R>(
    &self,
    consume: impl FnOnce(&[u8]) -> R,
) -> R
```

Both wrapper constructors are crate-private. The closure cannot retain a
borrowed secret, and the only production consumer is the HMAC digest builder in
`authoring-application-postgres`. The raw value never reaches SQLx arguments,
PostgreSQL, tracing, errors, receipts, audit rows, or `Debug` output.

### Two-phase authorized submission

Add `AuthorizedPromotionAccessV1<'a, E>`. It carries references to:

- `ProductRequestIdV1` through `ProductMutationContextV1`;
- `AuthenticatedActorV1`;
- current authenticated session fingerprint;
- `AuthorizedInstallationScopeV1`;
- fresh evidence `E`;

and owns:

- requested authoring session ID and generation;
- product idempotency secret.

Add `AuthorizedPromotionSubmissionV1<'a, E>`. It owns the access wrapper and the
typed `StartPromotionV1` produced from the atomic snapshot. It has private
fields, a crate-private constructor, redacted `Debug`, read-only accessors, and
the bounded secret-consumer method above.

Add `AuthorizedPromotionSubmissionPort<E>` with two methods:

```rust
async fn find_or_resume_authorized_promotion(
    &self,
    access: &AuthorizedPromotionAccessV1<'_, E>,
) -> Result<Option<PromotionSubmissionV1>, AuthorizedPromotionSubmissionErrorV1>;

async fn submit_authorized_promotion(
    &self,
    submission: AuthorizedPromotionSubmissionV1<'_, E>,
) -> Result<PromotionSubmissionV1, AuthorizedPromotionSubmissionErrorV1>;
```

`AuthoringApplication::promote_owned_session` takes `ProductRequestIdV1`
explicitly and performs this order:

1. authenticate credential and CSRF;
2. obtain current fresh Promote access to the installation;
3. call `find_or_resume_authorized_promotion` before reading the authoring
   snapshot;
4. return an exact final replay, or resume a durable partial admission, when
   found;
5. only on a true miss, load the atomic authorized snapshot;
6. build `StartPromotionV1` and call `submit_authorized_promotion`.

The prepare path repeats the replay check while holding database serialization
locks. A concurrent admission between steps three and six therefore converges
without requiring the old generation to remain current.

Exact replay authenticates the current principal and current product session,
requires current fresh Promote access to the installation, and then validates
the retained receipt, admission evidence, and promotion journal. It does not
require the admitted generation or admitted installation-authority revision to
still be current.

### Stable errors

The pure port exposes exactly:

```rust
pub enum AuthorizedPromotionSubmissionErrorV1 {
    NotFound,
    GenerationMismatch,
    Forbidden,
    ScopeMismatch,
    IdempotencyConflict,
    InvalidCandidate,
    PersistenceCorrupt,
    Indeterminate,
    Backend(AuthorizedPromotionBackendFailureV1),
}

pub enum AuthorizedPromotionBackendFailureV1 {
    Timeout,
    Retryable,
    Unavailable,
}
```

The PostgreSQL adapter maps `ProductDatabaseFailureV1` one-to-one to the backend
classification. Raw SQLSTATE, constraint names, SQL text, and driver messages
do not enter these errors.

The application maps `NotFound` non-enumeratingly, preserves
`GenerationMismatch`, maps stale or missing current Promote access to
`Forbidden` or `ScopeMismatch`, and preserves `Indeterminate` as retryable only
with the same product idempotency secret.

## Production adapter

Add `PostgresProductPromotions` under
`crates/authoring-application-postgres/src/product_promotions/`.

```text
product_promotions/
  mod.rs
  store.rs
  config.rs
  authorization.rs
  digest.rs
  admission.rs
  replay.rs
  publication.rs
  approval_environment.rs
  activation_link.rs
  row.rs
  readiness.rs
```

`store` owns the public adapter, dedicated pool, and high-level orchestrator.
`config` owns bounded timeouts and the shared product-action digest keyring.
`authorization` validates wrapper evidence and builds scalar SQL arguments.
`digest` derives HMAC aliases and semantic identities. `admission` owns the
immutable admission envelope. Each remaining module owns one database
capability and exact result decoding. `readiness` owns the database contract.

The adapter implements `AuthorizedPromotionSubmissionPort` only for
`FreshDiscordAuthorityEvidenceV1`.

## Dedicated execution model

`PostgresProductPromotions` is a dedicated high-level orchestrator. It does not
instantiate `PromotionService`, does not create request-scoped fake store ports,
and does not treat an `InvalidTransition` or revision conflict as an expected
control-flow signal.

The `authoring-promotion` crate exposes narrowly named pure calculations and
validators needed by both the legacy service and the new orchestrator:

- derive promotion identity from tenant, principal, and the domain key;
- build promotion intent separately from database admission time;
- materialize and validate a Prepared record using database time;
- validate a publication projection;
- transition Prepared to Published;
- derive and validate the product approval context and activation proposal;
- transition Published to ActivationPending or Expired;
- validate exact linked activation and final promotion projection.

These functions do no I/O and preserve all existing deny-unknown-fields and
digest semantics. The generic service is refactored to call them, so the domain
has one semantic implementation without making the generic service the
production transaction coordinator.

The normal production path is:

1. replay lookup;
2. prepare plus durable authenticated admission;
3. publish immutable inactive RuleSet and transition journal in one transaction;
4. read the exact approval environment;
5. derive the activation proposal in Rust;
6. create or reuse the activation, transition the journal, link it, and finalize
   receipt and audit in one transaction.

There is no second normal-path link verification call. Rust decodes and validates
each function's single result row before committing the surrounding transaction.
A malformed result rolls the transaction back.

No function changes `automation_ruleset_activations.active_version`.

## Admission, authorization, and recovery

### Current access versus historical admission

Every entry and every mutating stage requires a currently authenticated,
unrevoked product session for the same enabled principal and fresh current
Promote access to the same active tenant and installation. A later stage may use
a newer installation-authority revision than the original admission.

Only first admission additionally requires:

- the requested authoring session to be active and owned by the principal;
- the requested generation to be the current session head;
- the generation stage to be `preview_ready`;
- the generation candidate, binding, installation, guild, and RuleSet shadows to
  match the candidate intent;
- the generation's authority revision and digest to be the current installation
  authority;
- the fresh observation to match that current authority and remain valid under
  the database clock.

The successful Prepare transaction is the linearization point for this
current-head and current-authority decision. A head or authority change before
the row locks are obtained returns `GenerationMismatch` or `Forbidden` and
writes nothing. A change after commit does not revoke the accepted historical
promotion.

Publish, approval-environment resolution, activation/link, exact replay, and
partial recovery authenticate current access but use the immutable admitted
generation, candidate, policy, binding, and historical authority stored in the
admission and promotion journal. They do not compare those historical values to
the current authoring head.

PostgreSQL cannot authenticate Discord independently. Rust remains authoritative
for obtaining the observation. PostgreSQL proves that the submitted observation
matches the current installation authority, is fresh under a materialized
database clock, names `promote`, and carries guild-owner status or effective
`MANAGE_GUILD` or `ADMINISTRATOR` permission.

### Durable admission sidecar

Migration `202607200002_scope_product_promotion_execution.sql` adds nullable
legacy-compatible columns to `authoring_promotions`:

- `product_admission_format_version SMALLINT`;
- `product_admission_digest TEXT`;
- `product_admission JSONB`.

All three are null or non-null together. New product admission requires format
version one, a lowercase 64-character HMAC digest, an object no larger than
32,768 bytes, and an exact `ProductPromotionAdmissionEvidenceV1` envelope.

The envelope is exactly
`{format_version:1,payload:<ProductPromotionAdmissionPayloadV1>,admitted_at:<database time>}`.
Rust computes `product_admission_digest` over the canonical deny-unknown-fields
payload only. PostgreSQL never receives key material; it validates every payload
scalar against server state, supplies `admitted_at` from its materialized clock,
and stores the wrapper atomically. Database-owned admission time is immutable
and validated separately rather than included in the client HMAC.

The deny-unknown-fields envelope contains only:

- endpoint domain `product_promote_v1` and original product request ID;
- tenant, installation, principal, authoring session, generation, candidate,
  promotion ID, and promotion request digest;
- original session-subject digest;
- active idempotency HMAC, digest-key ID and key-material fingerprint;
- product semantic request digest and precomputed receipt and audit IDs;
- original Discord application, guild, acting user, Promote capability,
  authority revision and payload digest, observation digest and interval,
  effective permission bits, and owner flag;
- admitted candidate hash, binding fingerprint, and policy revision.

`expected_product_session_digest` is supplied to functions only to authenticate
the current database session row. It is never persisted. The audit field is
`session_subject_digest`: the existing domain-separated 32-byte SHA-256 subject
derived in Rust from tenant, principal, and authenticated session fingerprint.
Promotion reuses the approval and Apply domain and framing unchanged. The raw
fingerprint and raw database session digest are never stored in admission,
receipt, audit, or logs.

The admission HMAC covers the complete canonical payload. PostgreSQL validates
every scalar against server state before storing it, and the transition trigger
makes all admission columns immutable. Rust recomputes and constant-time checks
the payload HMAC and separately validates the database admission time whenever a
partial admission is resumed.

### Final product receipt

Admission evidence and the product action receipt have distinct meanings:

- admission proves that a specific current generation and authority were
  accepted at the Prepare linearization point;
- the immutable product receipt proves that the admitted promotion reached a
  linked `activation_pending` or terminal `expired` stage.

`product_action_receipts` and `product_audit_events` are therefore inserted only
by `starring_product_promotion_activation_link_v1` or the restricted legacy link
repair. Their final values are:

- endpoint domain `product_promote_v1`;
- audit action `promotion.promote`;
- target resource type `authoring_promotion`;
- final resulting state `activation_pending` or `expired`;
- result code `promotion_created` for a new admitted action or
  `promotion_recovered` for a legacy authenticated link repair;
- final promotion revision and HTTP disposition class `2`.

The final audit uses the original admitted request ID, session-subject digest,
and historical authority evidence. A recovery caller's current session digest
and current authority are access checks, not a rewrite of historical action
identity.

If a crash occurs after Prepare or Publish, the admission sidecar authorizes
safe continuation after current Promote access is re-established. If the final
transaction commits but its acknowledgement is lost, exact same-key replay finds
the immutable receipt and final journal.

### Legacy states

Rows created before this migration have null admission columns.

- legacy Prepared or Published rows are not silently adopted by the executor;
  they require an owner-run audited reconciliation because no authenticated
  product admission can be reconstructed;
- a legacy ActivationPending row with an exact product-authored activation may
  be repaired because the durable journal and activation already contain the
  complete action target;
- legacy repair still requires current authenticated Promote access, creates a
  version-one admission from the recovery request, verifies or performs the
  exact unlinked-to-linked transition, and atomically finalizes receipt and
  audit;
- an already-linked exact legacy activation may finalize its missing receipt and
  audit without mutating the link;
- any mismatch among journal, target, activation, receipt, or audit fails as
  `PersistenceCorrupt`.

The general executor cannot backfill arbitrary legacy admission evidence.

## Shared digest keyring

Extract and rename `ProductDecisionDigestKeyV1` and
`ProductDecisionDigestKeyringV1` to `ProductActionDigestKeyV1` and
`ProductActionDigestKeyringV1`. Approval and Apply use the shared types without
changing their existing digest domains or vectors. Temporary public type aliases
may preserve source compatibility for one release, but all new configuration
and promotion code uses the product-action names.

Promotion adds separate domain strings for:

- idempotency alias;
- semantic request;
- admission evidence;
- receipt ID;
- audit event ID.

The shared session-subject domain remains
`starring.product.session.subject.v1`; promotion does not introduce a keyed or
incompatible subject identity.

The configured keyring contains one active key followed by at most seven retired
keys. Raw keys and raw idempotency values are zeroizing and redacted. Exact replay
computes candidates under every retained key and adds at most one bounded alias
per retained key only when a final receipt exists.

A key cannot be removed while either a live `product_promote_v1` receipt or a
nonterminal admitted promotion depends on its key ID and material fingerprint.
Terminal promotion admission sidecars remain immutable historical evidence but
stop blocking key retirement after their receipt replay window expires.
After that window, the same raw key still derives the occupied deterministic
promotion ID but no longer receives a retained product receipt: it returns
`IdempotencyConflict` and can never create a second journal under that identity.

## Database capability contracts

All function arguments below are positional and exact. `AccessV1` is the shared
prefix, expanded in every listed `regprocedure` identity in this order:

```sql
expected_tenant_id TEXT,
expected_installation_id TEXT,
expected_principal_id TEXT,
expected_product_session_digest BYTEA,
expected_acting_user_id TEXT,
expected_discord_application_id TEXT,
expected_guild_id TEXT,
expected_capability TEXT,
observed_current_authority_revision BIGINT,
observed_current_authority_payload_digest TEXT,
authority_observation_digest TEXT,
authority_observed_at TIMESTAMPTZ,
authority_expires_at TIMESTAMPTZ,
effective_permission_bits TEXT,
guild_owner BOOLEAN
```

Every set-returning adapter query uses `SELECT * FROM ... LIMIT 2`. Exactly one
row is required. Zero rows, two rows, an unknown outcome, or malformed data is
`PersistenceCorrupt`.

### Identity

```text
public.starring_product_promotion_executor_database_identity_v1()
```

It returns scalar `TEXT`, uses `LANGUAGE sql`, and returns the shared nonzero
control-plane UUID text.

### Replay

```text
public.starring_product_promotion_replay_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,bigint,text,text[],text[],text[])
```

Arguments after `AccessV1` are:

```sql
expected_promotion_id TEXT,
expected_session_id TEXT,
expected_generation BIGINT,
semantic_request_digest TEXT,
idempotency_key_digest_candidates TEXT[],
idempotency_digest_key_id_candidates TEXT[],
idempotency_digest_key_fingerprint_candidates TEXT[]
```

It returns exactly:

```sql
TABLE(
    outcome_code TEXT,
    promotion_record JSONB,
    admission_evidence JSONB,
    receipt_projection JSONB,
    audit_evidence_projection JSONB,
    database_now TIMESTAMPTZ
)
```

`outcome_code` is `missing`, `partial_exact`, `final_exact`,
`idempotency_conflict`, `access_denied`, `scope_mismatch`, or
`persistence_corrupt`. Missing nullable projections are SQL `NULL` only for the
documented `missing` or partial state.

### Prepare

```text
public.starring_product_promotion_prepare_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bytea,text,bigint,bigint,text,text,text,text,jsonb,text,text[],text[],text[],text,text,text,text)
```

Arguments after `AccessV1` are:

```sql
product_request_id TEXT,
session_subject_digest BYTEA,
expected_session_id TEXT,
expected_generation BIGINT,
expected_candidate_revision BIGINT,
expected_candidate_hash TEXT,
expected_binding_fingerprint TEXT,
expected_promotion_id TEXT,
expected_promotion_request_digest TEXT,
prepared_promotion_intent JSONB,
active_idempotency_key_digest TEXT,
idempotency_key_digest_candidates TEXT[],
idempotency_digest_key_id_candidates TEXT[],
idempotency_digest_key_fingerprint_candidates TEXT[],
idempotency_digest_key_id TEXT,
semantic_request_digest TEXT,
new_receipt_id TEXT,
new_audit_event_id TEXT
```

It returns:

```sql
TABLE(
    outcome_code TEXT,
    promotion_record JSONB,
    admission_evidence JSONB,
    database_now TIMESTAMPTZ
)
```

The function materializes one database clock, performs replay lookup before
first-admission checks, locks and revalidates the current head and authority only
for a true miss, materializes the Prepared record with database time, inserts
the record and admission atomically, and returns `created`, `partial_exact`,
`final_exact`, `idempotency_conflict`, `generation_mismatch`, `access_denied`,
`scope_mismatch`, `invalid_candidate`, or `persistence_corrupt`.

### Publish

```text
public.starring_product_promotion_publish_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text)
```

Arguments after `AccessV1` are:

```sql
expected_promotion_id TEXT,
expected_promotion_revision BIGINT,
expected_promotion_request_digest TEXT,
expected_admission_digest TEXT
```

It returns:

```sql
TABLE(
    outcome_code TEXT,
    publication_projection JSONB,
    promotion_record JSONB,
    database_now TIMESTAMPTZ
)
```

It authenticates current access, validates historical admission, locks the exact
promotion and RuleSet head, and accepts only Prepared revision one or an exact
later replay. It computes `starring_ruleset_content_hash_v1`, reuses a version
only when schema version, JSONB definition semantic equality, and content hash
all match, otherwise rejects a collision. JSONB does not preserve input byte
representation, so byte-equivalence is not claimed. A new version is immutable
and inactive. Publication and Published revision two commit together.

### Approval environment

```text
public.starring_product_promotion_approval_environment_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text)
```

Arguments after `AccessV1` are identical to Publish. It returns:

```sql
TABLE(
    outcome_code TEXT,
    historical_binding_revision BIGINT,
    historical_resource_bindings JSONB,
    historical_binding_fingerprint TEXT,
    active_version BIGINT,
    active_content_hash TEXT,
    target_artifact_projection JSONB,
    database_now TIMESTAMPTZ
)
```

It authenticates current access, then reads only the admitted historical
authority, exact immutable target, and current active baseline. `active_version`
and `active_content_hash` are both null or both non-null. Rust recomputes the
resource and approval-binding fingerprints and rejects any mismatch.

### Activation and link

```text
public.starring_product_promotion_activation_link_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text,jsonb)
```

Arguments after `AccessV1` are:

```sql
expected_promotion_id TEXT,
expected_promotion_revision BIGINT,
expected_promotion_request_digest TEXT,
expected_admission_digest TEXT,
activation_proposal JSONB
```

It returns:

```sql
TABLE(
    outcome_code TEXT,
    promotion_record JSONB,
    activation_projection JSONB,
    receipt_projection JSONB,
    audit_evidence_projection JSONB,
    database_now TIMESTAMPTZ
)
```

The function authenticates current access and validates the persisted admission,
historical publication, proposal format version, target, requester, approval
payload and context digests, policy, binding, baseline, and TTL. It creates or
exact-reuses one unlinked product activation, advances the journal to
ActivationPending or Expired, performs the exact guarded link only after the
ActivationPending journal is visible in the transaction, and inserts final
receipt, aliases, audit, and audit evidence. All rows commit or none do.

An already-linked exact activation and final receipt returns `final_exact`.
There is no normal-path follow-up link call.

### Legacy activation-link repair

```text
public.starring_product_promotion_repair_link_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text,bytea,text,text[],text[],text[],text,text,text,text)
```

Arguments after `AccessV1` are:

```sql
expected_promotion_id TEXT,
expected_promotion_request_digest TEXT,
recovery_product_request_id TEXT,
recovery_session_subject_digest BYTEA,
active_idempotency_key_digest TEXT,
idempotency_key_digest_candidates TEXT[],
idempotency_digest_key_id_candidates TEXT[],
idempotency_digest_key_fingerprint_candidates TEXT[],
idempotency_digest_key_id TEXT,
semantic_request_digest TEXT,
new_receipt_id TEXT,
new_audit_event_id TEXT
```

It returns exactly the same six-column table contract as activation-link:

```sql
TABLE(
    outcome_code TEXT,
    promotion_record JSONB,
    activation_projection JSONB,
    receipt_projection JSONB,
    audit_evidence_projection JSONB,
    database_now TIMESTAMPTZ
)
```

It accepts only a migration-era ActivationPending journal with null admission
columns and an exact product-authored activation. It atomically installs the
version-one recovery admission, links if unlinked, and finalizes receipt and
audit. It cannot create a RuleSet version, create an activation, or adopt
Prepared or Published state.

### Keyring coverage

```text
public.starring_product_promotion_keyring_coverage_v1(text[],text[])
```

It returns `TABLE(outcome_code TEXT)`, uses `LANGUAGE sql`, and returns one of
`covered`, `missing_key`, or `ambiguous_keyring`. It examines final live
`product_promote_v1` receipts and nonterminal admission sidecars.

### Function metadata

Identity and keyring coverage use `LANGUAGE sql`; replay, prepare, publish,
approval environment, activation-link, and repair-link use `LANGUAGE plpgsql`.
Every external function is:

- `SECURITY DEFINER`;
- `VOLATILE`, `STRICT`, and `PARALLEL UNSAFE`;
- fixed to `SET search_path = pg_catalog`;
- non-leakproof, non-variadic, and without default arguments;
- owned by the common NOLOGIN control-plane owner;
- revoked from `PUBLIC` and every non-owner role before an executor grant.

Every table-returning function declares `ROWS 1`. The scalar identity function
has no rows estimate.

### Owner-only helpers

`starring_product_promotion_authorize_current_v1` is the sole shared current
access checker. Its exact argument identity is the 15-type `AccessV1` prefix and
its exact result is:

```sql
TABLE(
    outcome_code TEXT,
    database_now TIMESTAMPTZ,
    current_authority_projection JSONB
)
```

`starring_product_promotion_finalize_receipt_v1` accepts exact admission,
promotion, activation, receipt, and audit projections as five JSONB arguments
and returns `TABLE(outcome_code TEXT)`.

Both helpers are `LANGUAGE plpgsql`, `SECURITY DEFINER`, `VOLATILE`, `STRICT`,
`PARALLEL UNSAFE`, `ROWS 1`, owner-owned, revoked from `PUBLIC`, and granted to
no login role. External functions invoke them while already executing as the
common owner. Readiness fails if the executor can call either helper directly.

## Migration and shared invariants

All schema work is in exactly:

```text
migrations/202607200002_scope_product_promotion_execution.sql
```

The migration fails atomically before mutation if owners diverge, a required
constraint or trigger is absent, `public` is writable by another role, hostile
default privileges exist, an unsafe overload exists, or legacy rows violate
their current domain constraints.

It extends every shared receipt component from
`('product_approve_v1','product_apply_v1')` to include
`product_promote_v1`:

- endpoint constraints and endpoint-specific audit-action mapping;
- primary-alias and audit deferred assertions;
- audit-evidence capture and consistency checks;
- immutable receipt, alias, audit, and evidence trigger logic;
- alias capacity;
- seven-day replay retention indexes;
- `starring_purge_product_action_receipts_v1` selection, dependency checks, and
  bounded purge order;
- keyring coverage and key-retirement checks.

The purge function never deletes a final promotion receipt while its replay
guarantee is live, while dependent alias/audit/evidence rows are incomplete, or
while the corresponding promotion is nonterminal.

The migration installs an exact promotion transition trigger. It permits only:

- insert Prepared revision one;
- Prepared one to Published two;
- Published two to ActivationPending three;
- Published two to Expired three;
- ActivationPending three to Expired four.

Promotion ID, request digest, intent, tenant, installation, principal, creation
time, and admission columns are immutable. Updated time is monotonic. A stage,
revision, serialized stage tag, publication, activation, or admission mismatch
is rejected before write. Delete and truncate remain owner-maintenance-only and
cannot be reached by the executor.

The exact trigger manifest used by migration preflight, Rust readiness, and
security tests includes:

- `authoring_promotions_enforce_scope`;
- new `authoring_promotions_enforce_product_admission`;
- new `authoring_promotions_enforce_product_transition`;
- `automation_ruleset_versions_reject_mutation`;
- `automation_ruleset_versions_reject_truncate`;
- `activation_requests_enforce_product_journal_link`;
- `activation_requests_enforce_product_scope`;
- `activation_requests_guard_legacy_product_slot`;
- `activation_requests_guard_ruleset_artifact_transition`;
- `product_action_receipts_assert_approval_alias` with promote-aware body;
- `product_action_receipts_assert_approval_audit` with promote-aware body;
- `product_action_receipts_reject_mutation`;
- `product_action_receipt_idempotency_aliases_enforce_capacity`;
- `product_action_receipt_idempotency_aliases_reject_mutation`;
- `product_audit_events_capture_receipt_evidence`;
- `product_audit_events_reject_mutation`;
- `product_action_receipt_audit_evidence_reject_mutation`.

Readiness compares normalized complete trigger definitions and function
identities, not names alone.

## Exact relation manifest

The production capability directly or transitively touches exactly these 17
ordinary relations:

1. `public.product_control_plane_identity`;
2. `public.product_principals`;
3. `public.product_auth_sessions`;
4. `public.product_tenants`;
5. `public.automation_installations`;
6. `public.automation_installation_authority_versions`;
7. `public.authoring_sessions`;
8. `public.authoring_session_generations`;
9. `public.authoring_promotions`;
10. `public.automation_ruleset_heads`;
11. `public.automation_ruleset_versions`;
12. `public.automation_ruleset_activations`;
13. `public.activation_requests`;
14. `public.product_action_receipts`;
15. `public.product_action_receipt_idempotency_aliases`;
16. `public.product_audit_events`;
17. `public.product_action_receipt_audit_evidence`.

All 17 must be ordinary tables, share the same common owner as the functions,
and have no executor table or column privilege. The existing tables use ordinary
owner-enforced invariants rather than RLS; readiness requires RLS to remain
disabled so an unexpected policy cannot silently alter SECURITY DEFINER
semantics. Trigger helper functions are owner-only and are not part of the
executor allowlist.

The executor receives no schema or database create, temporary-object privilege,
role membership, owner membership, bypass RLS, grant option, or unrelated
`starring_*` execution.

## Readiness

`PostgresProductPromotions::verify_readiness` runs before ingress with bounded
read-only and rollback-only probes.

It verifies:

- exact external and internal function identity, result, language, volatility,
  strictness, security mode, parallel safety, search path, owner, rows estimate,
  argument names and types, and overload set;
- shared database identity and a direct session role;
- common NOLOGIN non-member ownership for all functions, relations, and trigger
  helpers;
- the exact eight-function executable allowlist and absence of helper execution;
- all 17 relation contracts and absence of table or column privilege;
- no database create, temporary, schema create, membership, superuser,
  create-role, create-database, replication, bypass-RLS, or grant option;
- trusted public schema and default-privilege contracts;
- exact constraints and the complete trigger manifest;
- keyring coverage for live receipts and nonterminal admissions;
- invalid authorization, malformed digest, stale generation, hostile JSON,
  impossible scope, and duplicate-result probes leave no data.

Readiness reports only contract mismatch, missing capability, excessive
capability, incomplete keyring coverage, invalid probe result, or classified
database failure.

## Result integrity and size bounds

Every promotion, admission, publication, approval-environment, activation,
receipt, and audit JSON envelope has an explicit format version and Rust
`serde(deny_unknown_fields)` decoding. Domain `PromotionRecordV1::validate` runs
on every loaded and returned journal.

Bounds are exact:

- promotion record JSONB text: at most 8,388,608 bytes;
- RuleSet definition JSONB text: at most 524,288 bytes;
- admission JSONB text: at most 32,768 bytes;
- authority resource bindings JSONB text: at most 262,144 bytes;
- activation proposal and projection JSONB text: at most 1,048,576 bytes;
- receipt or audit projection JSONB text: at most 65,536 bytes;
- function rows: exactly one, queried with `LIMIT 2`.

Rust recomputes and validates:

- promotion and request identities;
- admitted evidence HMAC;
- RuleSet canonical content hash;
- schema version and JSON semantic definition equality;
- publication identity and inactive status;
- resource and approval-binding fingerprints;
- approval policy, payload, context, activation, receipt, and audit digests;
- exact journal-to-activation link.

Unexpected fields, oversized values, invalid UTF-8 boundaries, noncanonical
arrays, missing paired nullable fields, unknown outcomes, or a second row fail
closed before transaction commit.

## Transactions, lock order, and retry

Every stage begins an explicit bounded transaction and sets statement, lock,
idle-in-transaction, and safe search-path settings locally. Normal mutation uses
Read Committed isolation plus explicit row locks. Readiness uses its existing
probe isolation modes.

All functions follow this global order:

1. inspect candidate idempotency aliases and receipts under MVCC without locking
   immutable rows;
2. lock mutable principal and current product-session rows only as required for
   authentication;
3. lock tenant, installation, and current installation-authority head;
4. on first admission only, lock the authoring session head and read the
   immutable generation and historical authority without unnecessary row locks;
5. lock the promotion row;
6. lock the RuleSet head, then read immutable version and active pointer;
7. lock the exact activation request when it exists;
8. insert receipt, aliases, audit, and evidence after all action locks.

No path acquires an earlier class after a later class. Immutable RuleSet
versions, authoring generations, completed receipts, audits, aliases, and audit
evidence are validated but not locked merely for reading.

Two concurrent exact submissions converge to one admission, one promotion, one
immutable version, one activation, one link, one receipt, and one audit event.
Changed semantic input under the same product key returns
`IdempotencyConflict` before publication.

Automatic transaction retry is bounded and allowed only for SQLSTATE `40001`
and `40P01`, which prove the transaction rolled back. Lock timeout, statement
timeout, capacity, and availability return their stable backend class. A
transport failure during or after COMMIT is `Indeterminate`; resolution is only
same-key replay. A new key is never used to guess commit success.

## Tests

### Pure application

- exact final replay is checked before snapshot load;
- partial admission resumes before snapshot load;
- true miss loads one atomic snapshot and creates one submission wrapper;
- a concurrent admission between lookup and Prepare converges;
- request ID, current actor, session fingerprint, scope, fresh evidence, raw-key
  HMAC inputs, session, generation, and start input reach the authorized port;
- generic `PromotionService` does not satisfy the authorized port;
- wrong scope, owner, generation, requester, or authority never reaches first
  admission;
- redacted Debug contains no credential, idempotency secret, session
  fingerprint, RuleSet body, or evidence payload;
- every stable error mapping is exhaustive.

### PostgreSQL semantics

- happy submission reaches one exact linked ActivationPending record and final
  receipt;
- publication remains inactive;
- final exact replay does not require the old authoring head or old authority to
  remain current;
- current access is still required for every replay and recovery;
- current head or authority drift before Prepare writes nothing;
- drift after Prepare does not invalidate durable admission;
- exact replay is projection-stable and creates no duplicate durable rows;
- changed session, generation, candidate, RuleSet, binding, policy, actor, or
  scope under the same key conflicts or fails closed;
- concurrent exact submissions converge under the documented lock order;
- disconnect after Prepare or Publish resumes from admission;
- unknown activation-link commit resolves through receipt replay;
- Expired produces the exact terminal journal and final receipt;
- legacy exact ActivationPending unlinked and linked rows finalize through only
  the repair function;
- legacy Prepared and Published rows are refused;
- corrupt scalar shadows, admission, JSON, canonical hash, link, receipt, alias,
  audit, or evidence are rejected;
- each result decoder rejects zero rows, two rows, unknown outcomes, oversized
  records, and unknown JSON fields.

### Shared retention and key rotation

- promote receipts receive the same seven-day replay guarantee as approval and
  Apply;
- alias, audit, and evidence deferred triggers require complete promote rows;
- bounded purge removes dependencies in the documented order only after the
  replay window;
- a nonterminal admission blocks removal of its digest key;
- a live final receipt blocks removal of its digest key;
- a terminal expired receipt and historical admission stop blocking after purge;
- approval and Apply digest vectors and retention behavior do not change.

### Restricted-role security

- direct SELECT, INSERT, UPDATE, DELETE, TRUNCATE, REFERENCES, TRIGGER, and column
  access fail on all 17 relations;
- unrelated product, approval, Apply, status, identity, retention, runtime, and
  helper functions cannot be executed;
- only the exact eight external functions execute;
- SET ROLE, owner membership, grant option, temporary objects, schema create,
  and database create fail;
- PUBLIC, inherited, column, default-privilege, overload, owner, trigger,
  search-path, RLS, and schema-trust drift fail readiness;
- hostile migration prerequisites roll back without residue;
- a direct-login restricted role completes and replays one exact authorized
  promotion.

### Regression gates

- existing promotion domain and PostgreSQL recovery suites;
- activation link, approval context, approval, Apply, status, and runtime
  convergence suites;
- workspace tests, all-targets build, Clippy with warnings denied, and format;
- dependency, no-model-gateway, no-comment, package, secret, and JavaScript
  gates;
- exact GitHub Actions PostgreSQL command matrix.

## Performance and product behavior

The path removes production general-store calls and completes a new request with
one replay lookup, one Prepare transaction, one Publish transaction, one bounded
approval-environment read, and one ActivationLink transaction. The link is not
verified through a second normal-path database call.

Final replay normally uses one replay function call after current authorization.
Partial replay uses replay plus only the remaining stages. Each function returns
one bounded row, and no function returns encrypted snapshots or unrelated
installation data.

No commercial latency claim is made from unit tests. Production-shaped repeated
measurements report p50, p95, and p99 separately for new admission, final replay,
Prepared recovery, Published recovery, and legacy link repair. Database time is
separated from snapshot decryption and Discord authority latency.

## Rollout

1. drain product promotion traffic;
2. run `202607200002_scope_product_promotion_execution.sql` as the common owner;
3. inventory legacy partial rows and reconcile Prepared or Published rows through
   an owner-only audited procedure;
4. create a dedicated direct-login executor outside migrations;
5. grant only the exact eight-function allowlist;
6. configure one dedicated bounded pool and the shared retained keyring;
7. pass component readiness and restricted-role E2E;
8. switch product promotion composition from generic raw stores;
9. prove final replay, partial recovery, and unknown-commit recovery;
10. remove the API process's legacy promotion, RuleSet publication, and
    activation table credentials;
11. only then compose the closed HTTP facade.

The old generic adapters remain valid for tests and explicit manual tooling but
are not valid production credentials.

## Non-goals and following work

This slice does not implement rejection, the production approval-environment
provider used by Apply, the authenticated snapshot cipher, the concrete HTTP
facade, `tools/starring-api`, the trusted authoring-generation writer,
`tools/starring-runtime`, or public Cloudflare ingress.

After this capability, the order remains:

1. production approval environment, snapshot cipher, and rejection adapters;
2. aggregate product-role readiness and concrete `ProductControlFacade`;
3. loopback-only runnable API with graceful shutdown;
4. trusted Luna authoring-generation writer and conversational endpoint;
5. runtime worker and exact Live-loss recovery;
6. release, backup, restore, crash, and commercial SLO evidence.
