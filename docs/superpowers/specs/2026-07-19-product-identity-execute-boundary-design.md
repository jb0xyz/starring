# Product identity execute-only boundary

Date: 2026-07-19

## Objective

Move every product identity lifecycle mutation behind versioned PostgreSQL capabilities while preserving the existing OAuth causality, session secrecy, failure classification, concurrency, and constant-time CSRF behavior.

The boundary must support production role separation. A caller that can start or consume an OAuth flow must not automatically be able to mint a session. A normal product API caller must not be able to mint identities or perform security revocation. A retention caller must not be able to authenticate users.

## Trust boundary

`VerifiedDiscordIdentityV1` is an in-process capability. PostgreSQL cannot independently prove that a Discord code exchange and identity lookup produced it.

The database boundary therefore limits each credential to one operation family:

| Pool | Allowed capabilities | Explicitly excluded |
| --- | --- | --- |
| OAuth flow writer | flow create, flow consume | session issue, session API, security revoke, retention |
| Session issuer | session issue | flow create or consume, session API, security revoke, retention |
| Session API | session read, CSRF mutation read, touch, logout read and commit | flow mutation, session issue, security revoke, retention |
| Security revoker | security revoke | OAuth flow mutation, session issue, session API, retention |
| Retention | identity purge | every request-serving capability |

The session issuer requires both its dedicated database credential and an unguessable consumed-flow digest supplied by the verified callback path. A full process compromise can still obtain both. A later stronger boundary may bind a signed Discord verification receipt to the flow and verify that receipt in PostgreSQL.

All four pools must resolve to the same logical control-plane database. Migration 017 creates one non-secret UUID identity in `product_control_plane_identity` and four role-specific read capabilities. Aggregate readiness compares that identity and `current_database()` across all pools. A separately migrated database therefore cannot be mixed into one store even when its schema and grants are otherwise identical. An independent environment restored from a backup must receive a new identity before ingress; a failover member of the same logical database retains it.

## Database capabilities

All functions are versioned, `SECURITY DEFINER`, `VOLATILE`, `STRICT`, `PARALLEL UNSAFE`, fixed to `search_path=pg_catalog`, owned by the common owner of the three legacy identity relations, and closed to `PUBLIC` and unexpected named grantees. The new control-plane identity relation is transferred to that same owner.

### OAuth flow writer

`starring_product_oauth_database_identity_v1()` returns the logical database identity used only by readiness.

`starring_product_oauth_flow_create_v1(bytea, bytea, text, text, double precision)` returns a closed outcome plus the persisted expiry and database clock. Outcomes are `created`, `exact_replay`, `digest_conflict`, and `invalid_request`.

An exact replay requires both digests, redirect URI, and return path to match one unconsumed and unexpired row. Digest conflicts never reveal the conflicting row.

`starring_product_oauth_flow_consume_v1(bytea, bytea, text, text[])` returns `claimed` with the exact flow projection, or `invalid_or_consumed` with no projection. It locks the flow before reading the database clock. Consumption remains single-winner and intentionally has no successful replay.

### Session issuer

`starring_product_session_issuer_database_identity_v1()` returns the same logical database identity through an issuer-only capability.

`starring_product_session_issue_v1(bytea, text, text, timestamptz, text, text, bytea, bytea, double precision, double precision)` accepts the exact consumed-flow projection, verified Discord projection, new session and CSRF digests, and bounded lifetimes.

The function derives `discord:<snowflake>` itself. It locks the flow before evaluating expiry, rejects noncanonical existing principal mappings, updates the principal, and inserts the session atomically. Outcomes are `issued`, `exact_replay`, `flow_invalid_or_consumed`, `principal_disabled`, `digest_conflict`, `invalid_request`, and `invariant`.

Exact replay requires the same flow, session digest, CSRF digest, and canonical principal. It does not advance the identity revision a second time. A different session bound to the flow is `flow_invalid_or_consumed`. A session or CSRF collision on another flow is `digest_conflict` and may generate a new secret pair. Other unique or integrity failures fail closed.

Migration 018 makes commit reconciliation independent of the OAuth flow's current expiry without weakening initial issuance. The function locks the exact consumed flow and looks for its bound session before applying the current-time expiry gate. An existing session is an `exact_replay` after flow expiry only when the flow consumption preceded or equaled session authentication, session authentication occurred strictly before flow expiry, both credential digests and the canonical principal match, both requested lifetimes reproduce the persisted expiries exactly, the session remains unrevised and unrevoked, and the principal and session projections satisfy every checked invariant. A missing session still requires the database clock to remain strictly before flow expiry. Post-expiry replay therefore proves a previously committed issuance; it cannot create a session or extend the OAuth authorization window.

### Session API

`starring_product_session_api_database_identity_v1()` returns the same logical database identity through a session-API-only capability.

Existing migration 015 functions remain the only current-principal and CSRF-authentication read surface.

`starring_product_session_logout_read_v1(bytea)` locks the session and returns only lengths, revocation metadata, and `sha256(session_digest || stored_csrf_digest)`. Rust computes the same tag from the submitted CSRF digest and compares it with `subtle` before any state classification.

`starring_product_session_logout_commit_v1(bytea, bytea, timestamptz)` performs a compare-and-set user logout using the session-bound comparison tag and observed activity timestamp. It never accepts a caller-selected reason.

### Security revoker

`starring_product_security_revoker_database_identity_v1()` returns the same logical database identity through a revoker-only capability.

`starring_product_session_security_revoke_v1(bytea)` performs only `security_revocation`. Outcomes are `revoked`, `exact_replay`, `already_revoked`, `invalid_credential`, and `invariant`.

## Transaction and concurrency contract

Every identity write starts an explicit `READ COMMITTED, READ WRITE` transaction and sets transaction-local statement, lock, and idle-in-transaction timeouts to the same bounded value.

The flow consume and session issue locks are acquired before `clock_timestamp()` is evaluated. A request that waits beyond expiry fails after acquiring the lock. Concurrent flow consumption has one winner. Concurrent session issue for one flow has one winner, and a losing principal mutation is rolled back. Principal revisions for independent logins serialize through the principal row.

Logout keeps its row lock across the Rust constant-time comparison and compare-and-set mutation. Concurrent equal logout requests produce `Revoked` and `ExactReplay`. Logout and security revocation preserve the first committed reason; the other operation reports an already-revoked result.

## Error and secret contract

Expected domain failures use closed outcome codes. Unknown codes, malformed success projections, noncanonical principal data, and unexpected constraint identities are invariants or redacted backend failures.

Only exact flow state and nonce constraint collisions and exact session or CSRF constraint collisions are eligible for bounded secret regeneration. Generic `23505` handling is forbidden.

Stored state, nonce, session, CSRF, and OAuth binding digests are never returned. The logout comparison tag is session-bound and is compared in Rust with constant-time equality.

After a session-issue commit error, the adapter makes exactly one reconciliation attempt with the same raw session and CSRF credentials and every other immutable input unchanged. It never generates another credential pair after the outcome becomes uncertain. Only a fully validated `issued` or `exact_replay` response resolves that attempt; a transaction failure, second commit error, digest conflict, domain rejection, or malformed projection preserves `CommitIndeterminate`. Flow consumption is never reconciled as a successful replay because the Discord authorization code exchange sits after that boundary.

## Readiness and rollout

Readiness is capability-specific for the four request-serving pools. It verifies exact function identity, result, language, volatility, strictness, parallel safety, security-definer status, fixed search path, row estimate, owner, ACLs, relation ownership, RLS state, direct relation privileges, every table-level and column-level relation ACL grantee, direct-login role properties, and rollback-only functional probes. Aggregate readiness additionally requires four distinct role names and one exact logical database identity and database name.

Migration 017 preserves pre-existing grants on the three legacy identity relations because decision and status reads are not function-scoped yet. Their presence intentionally makes the new aggregate readiness fail. A later sealing migration must revoke every non-owner table and column grant only after those remaining routes move behind exact functions. Migration 017 itself normalizes the new control-plane identity relation and all identity lifecycle, transition, and purge functions. This slice is not independently eligible for production ingress while the global relation ACL gate is red.

Migration 018 replaces only the session-issue capability and repeats the migration-017 owner, function-shape, fixed-search-path, and ACL normalization. It removes `PUBLIC` and every named non-owner grant, including grants inherited from hostile function default privileges. The issuer's exact execute grant must be restored after the migration and aggregate readiness must pass before ingress resumes.

The final process readiness layer must additionally compare every caller's complete `public.starring_*` executable function set against a role manifest. Component readiness alone cannot detect an unrelated extra function grant.

Migration also normalizes ownership and ACLs for the control-plane identity relation, its four topology functions, the identity transition trigger functions, and `starring_purge_product_identity_v1`. The production role bootstrap grants each capability only after migrations and readiness contracts are present. An independent backup clone must receive a new logical database UUID before it becomes a separate environment; members of one failover topology retain one UUID.

Release gates include unit tests, dependency guards, real PostgreSQL lifecycle semantics, direct-login least-privilege tests, mixed-database pool rejection, unrelated-role table and column ACL rejection, migration rollback and hostile-default-grant tests, workspace tests, clippy with warnings denied, and formatting. Production ingress additionally requires the later legacy relation ACL sealing gate.
