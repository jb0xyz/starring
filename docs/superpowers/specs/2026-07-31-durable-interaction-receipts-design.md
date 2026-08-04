# Durable Interaction Receipts Design

Date: 2026-07-31

Status: accepted implementation design for C1

## Outcome

C1 makes every production Discord interaction delivery durable before Starring
sends an acknowledgement, response, follow-up, or mutation. Concurrent and
replayed deliveries of the same Discord interaction have one executor. Every
non-owner duplicate performs zero Discord HTTP calls.

C1 does not claim complete external-effect reconciliation. C2 adds complete
deterministic preflight and C3 adds the per-effect journal, observation, and
compensation needed to resume ambiguous mutations.

## Boundaries

The pure `automation-runtime-interaction` crate owns validated receipt types,
canonical digests, state transitions, claim dispositions, token AAD, and token
envelope cryptography. It has no SQL, Twilight, HTTP, or product-control
dependency.

`automation-runtime-interaction-postgres` owns the restricted PostgreSQL
capabilities. It derives tenant, installation, and deployment from current
authoritative runtime rows and never trusts caller-supplied product scope.

`automation-runtime` owns preparation and execution orchestration. It cannot
construct a responder or mutation permit until a durable receipt claim exists.

`starring-runtime` resolves the dedicated token-envelope keyring and composes
the production receipt port. Key material and Discord tokens remain absent from
configuration files, source, logs, errors, metrics, and evidence.

## Identity and immutable binding

The receipt key is the exact pair of Discord application ID and interaction
ID. The immutable root binds the canonical semantic request digest and exact
serving route:

- tenant, installation, and deployment derived by PostgreSQL;
- guild, RuleSet key, version, content hash, binding revision, and binding
  fingerprint;
- runtime generation, process instance, serving lease, gateway owner, and
  route fencing identity;
- static or instance route kind and exact instance identity when applicable;
- canonical request digest.

The request digest excludes the interaction token. It includes the application,
interaction, guild, channel, actor, interaction kind, custom ID, locale, and
canonically ordered modal input identifiers and values. A replay with the same
receipt key and any different immutable binding is corruption and fails closed.

The deterministic action-plan digest is set exactly once through a later CAS
before the first mutation. Its versioned canonical projection binds action
order, action kind, typed parameters, references, normalized execution context,
and acknowledgement strategy. Debug formatting and incidental JSON object
ordering are not digest inputs.

## Two-stage execution

The production flow is:

```text
normalize
→ reserve global admission
→ admit exact serving route
→ encrypt interaction token
→ durable receipt claim
→ duplicate disposition or exclusive claim
→ durable first-response intent
→ optional Discord acknowledgement or defer
→ deterministic preparation
→ bind action-plan digest with CAS
→ durable external-execution intent
→ execute through receipt-fenced adapters
→ durable terminal transition and token-secret deletion
```

The claim precedes every Discord HTTP call. Preparation may happen before the
first response when it is bounded and local. Instance preparation that needs a
fresh Discord observation may occur after a durable defer so it does not consume
the entire initial-response budget. No mutation occurs before the exact plan
digest is bound.

Receipt claim has an absolute deadline covering pool acquisition, transaction,
authority checks, insert, and commit. The initial candidate is 600 milliseconds,
inside Discord's three-second initial-response budget. A timeout, unavailable
database, or indeterminate claim produces zero Discord calls.

## Durable state

The immutable root and guarded mutable head are separate from the short-lived
token secret. The execution head uses these states:

```text
claimed
acknowledging
deferred
prepared
executing
completed
failed
recovery_required
```

The acknowledgement head is independently observable as unacknowledged,
attempting, deferred, responded, or response-recovery-terminal. Recording an
attempt before a Discord call prevents a restart from confusing an unsent call
with a call that may have succeeded remotely.

`failed` is allowed only for a known failure with no ambiguous external effect.
Timeout, disconnect, cancellation, or persistence failure after an external
attempt becomes `recovery_required`. C1 does not replay an executing mutation.

`terminal_duplicate` is a claim disposition, not a persisted replacement for
the original receipt state. Supported dispositions are acquired, duplicate
completed, duplicate terminal, duplicate in-flight, recovery required, and
semantic corruption.

## Claim ownership and fencing

Every mutation of the receipt head checks the receipt revision, current process,
current gateway owner, current serving lease, and exact route fence. A stale
process cannot acknowledge, bind a plan, execute, complete, fail, or delete the
token secret.

An expired `claimed` receipt proves that no external intent was recorded and may
be reclaimed only by exact current authority while its token remains valid. An
expired acknowledgement attempt, deferred receipt, prepared receipt, or
executing receipt is never blindly re-executed. It moves to
`recovery_required` pending exact observation.

## Token envelope

The interaction token is encrypted with XChaCha20-Poly1305 and a fresh 24-byte
nonce. A dedicated active-and-retired keyring is resolved from:

```text
STARRING_RUNTIME_INTERACTION_TOKEN_ENVELOPE_KEYRING_SECRET_REFERENCE
```

The staging Keychain identity is:

```text
keychain:starring.runtime.staging:interaction.token-envelope-keyring
```

The versioned AAD binds cipher suite and key ID, receipt key, application and
interaction IDs, derived tenant, installation, and deployment, exact route,
request digest, and absolute expiry. Key ID, nonce, ciphertext, AAD, and expiry
tampering all fail authentication.

Ciphertext lives in a separate relation with the receipt key, key ID, nonce,
AAD version, expiry, and creation time. Terminal completion or bounded expiry
deletes it through one narrow security-definer capability. The immutable receipt
history remains. An unknown key, invalid ciphertext, missing secret, or expired
token terminalizes response recovery without inventing success.

## PostgreSQL authority

The interaction role has no direct relation or sequence access. Its only new
capabilities are narrow security-definer functions for claim, observation,
first-response intent/result, plan binding, execution intent, terminal
transition, expired recovery observation, and token-secret expiry.

Claim performs these checks and writes in one transaction:

1. validate bounded canonical input;
2. verify current gateway owner and process identity;
3. verify the exact current Live serving lease, deployment, target, attestation,
   and route fence;
4. derive tenant, installation, and deployment identity;
5. classify an existing receipt as exact duplicate or corruption;
6. insert immutable receipt, mutable head, and encrypted token secret atomically;
7. issue claim and token expiries from database time.

No transaction or row lock is held across a Discord call.

## Failure semantics

- Claim failure or claim timeout: zero external calls.
- Completed duplicate: zero external calls.
- Non-owner in-flight duplicate: zero external calls.
- Same key with different semantic identity: corruption, zero external calls.
- First-response attempt with stale authority or expired token: rejected before
  HTTP.
- Crash before external intent: exact bounded reclaim may resume.
- Crash after external intent: recovery required; no blind replay.
- Mutation success followed by unknown DB completion: recovery required until
  C3 exact observation.
- Missing or corrupt token after restart: response recovery terminal, mutation
  reconciliation truth preserved.

Malformed, paused, stale, or unknown Starring routes that cannot obtain an
authoritative receipt are dropped without a courtesy Discord response. Foreign
custom IDs remain ignored.

## Acceptance

C1 is complete only when focused tests prove:

- concurrent identical deliveries produce one executor;
- completed and non-owner duplicates produce zero Discord HTTP calls;
- request or route drift under the same receipt key fails closed;
- stale process and route fences cannot transition or execute;
- database outage and claim timeout produce zero Discord calls;
- restart checkpoints either safely resume an unattempted claim or truthfully
  terminalize to recovery required;
- nonce, ciphertext, key, AAD, and expiry tampering fails authentication;
- expired tokens cannot authorize a first response;
- secrets never appear in Debug, errors, logs, metrics, HTTP evidence, or test
  artifacts;
- direct table access and unrelated function execution fail;
- existing workspace tests, clippy with warnings denied, formatting, migration,
  readiness, and restricted-role gates remain green.
