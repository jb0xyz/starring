# Product decision reader capability

Date: 2026-07-19

## Objective

Replace the product-decision adapter's direct 11-relation query with one
versioned PostgreSQL read capability. Preserve the existing application ports,
domain validation, non-enumerating `NotFound` behavior, scope checks, authority
freshness, decision phases, and exact deployment projection.

This slice makes the decision-reader credential independently least-privileged.
It does not complete Apply scoping, remove legacy grants needed by other
adapters, or make the whole process eligible for production ingress.

## Boundary

The reader role executes exactly:

- `starring_product_decision_reader_database_identity_v1()`;
- `starring_product_decision_read_v1(TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, BYTEA)`.

The read function inputs are promotion ID, tenant ID, installation ID, Discord
guild ID, authenticated principal ID, acting Discord user ID, and the opaque
product-session digest. All inputs are non-secret except the digest, which is
already an irreversible 32-byte server-side session identity. The function
never accepts raw session or OAuth credentials.

The database function returns zero, one, or at most two internal rows. Zero rows
covers missing, wrong-scope, wrong-principal, wrong-Discord-user, wrong-session,
malformed, and non-product activation inputs uniformly. Rust maps zero rows to
`ProductControlPortError::NotFound`, exposes exactly one validated projection,
and rejects two rows as persisted corruption. The database function does not
return a reason that could become an enumeration oracle.

## Projection contract

The function returns the existing 49-column `ProductDecisionRow` contract in
the existing names and types:

| Group | Fields |
| --- | --- |
| Activation | request, tenant, installation, guild, ruleset, requester, quorum, state, creation/expiry, promotion and approval digests, approval context, product revision, approval count |
| Promotion | tenant, stage, request digest, canonical record |
| Current installation | tenant lifecycle, Discord application/guild, ruleset, installation lifecycle, current authority revision and digest |
| Promoted authoring generation | session owner, Discord owner, session/generation, generation stage, candidate revision/hash, resource bindings and fingerprint |
| Historical authority | binding revision, bindings/fingerprint, policy revision, quorum and activation TTL |
| Request actor | Discord user, disabled flag, session revocation and expiry bounds |
| Runtime | optional deployment ID and desired-target digest |
| Clock | database statement timestamp |

The database only projects persisted evidence. Rust continues to parse and
validate the promotion record, recompute payload and binding fingerprints,
validate current and historical authority, enforce the capability-specific
freshness window, derive the public phase, and construct domain types. Moving
those checks into SQL would duplicate the domain engine and make future schema
changes harder to review.

Disabled, revoked, or expired actors and inactive tenant or installation state
continue to return a row for Rust to classify as `InvalidState`. Persisted
semantic corruption continues to become a redacted backend failure. Only
identity and target selection failures collapse to `NotFound`, so this boundary
does not accidentally change the established public decision-state contract.

## Input and query invariants

The function is `STRICT` and rejects work early unless:

- the promotion ID is exactly 64 lowercase hexadecimal characters;
- tenant, installation, and principal IDs match the persisted 1–128 character
  canonical identifier grammar;
- the guild ID is a canonical nonzero unsigned 64-bit decimal value;
- the acting Discord user ID is a canonical nonzero unsigned 64-bit decimal
  value;
- the session digest is exactly 32 bytes.

The query retains every existing join predicate and its `product_authoring`
authority-kind filter. Principal, acting Discord user, and session digest are
all bound before a row can be returned. It uses fully schema-qualified
application relations and `search_path=pg_catalog`. It remains one database
snapshot and obtains one statement timestamp so Rust authority validation
observes one coherent clock. The function returns at most two rows: Rust
accepts exactly one and treats two rows as a redacted persistence-integrity
failure instead of silently selecting one.

## Relation manifest

The function directly reads 11 ordinary relations:

1. `activation_requests`;
2. `activation_request_approvals`;
3. `authoring_promotions`;
4. `product_tenants`;
5. `automation_installations`;
6. `automation_installation_authority_versions`;
7. `authoring_sessions`;
8. `authoring_session_generations`;
9. `product_principals`;
10. `product_auth_sessions`;
11. `runtime_deployments`.

Migration and readiness require those 11 data relations plus
`product_control_plane_identity` to be 12 ordinary, non-RLS tables with the same
reviewed owner as both reader functions. The dedicated caller receives no table
or column privilege. Existing non-owner relation grants are preserved by the
migration because other transitional adapters still use direct SQL, but any
such grant on this manifest keeps reader component readiness red.

This is a database least-privilege boundary, not a cryptographic authorization
proof. PostgreSQL cannot verify the in-process fresh Discord authority evidence;
the typed application boundary remains responsible for that authorization. A
stolen reader credential combined with a valid product-session digest is
therefore outside this slice's guarantee and remains part of the credential
isolation and final ingress threat model.

## Function and ACL contract

`starring_product_decision_read_v1` is a SQL, `VOLATILE`, `STRICT`,
`PARALLEL UNSAFE`, `SECURITY DEFINER` set-returning function with
`search_path=pg_catalog` and `ROWS 1`. The common owner is a restricted
`NOLOGIN` role. Migration strips `PUBLIC`, named-role, grant-option, and any
effective grants inherited from hostile default privileges from both current
reader functions before production grants are applied. It does not rewrite the
underlying default-privilege policy, which must be audited and restricted
separately before ingress.

Reader readiness checks exact signature, return shape, language, volatility,
strictness, parallel mode, row estimate, owner, search path, relation shape,
RLS state, global table and column ACLs on all 12 relations, and the direct-login
role contract. It also computes the caller's exact executable allowlist across
every public `starring_*` routine and public security-definer routine.

## Runtime transaction

Every request uses the decision-reader pool and a read-committed, read-only
transaction. Statement, lock, and idle-in-transaction timeouts are local to the
transaction. Its search path is fixed to `pg_catalog`. The capability call is
the only data-reading statement and therefore its statement snapshot is the
read linearization point; configuration statements cannot pin an older
repeatable-read snapshot. A database error is classified and redacted through
the existing product-control backend mapping.

Readiness executes two bounded checks in one read-only transaction:

1. the topology function must return the expected logical database UUID,
   database name, and direct session role;
2. a canonical but impossible seven-input tuple must return exactly zero rows.

The impossible probe proves capability execution for one data-independent
zero-row tuple. Query predicates and the restricted-role end-to-end mismatch
matrix prove uniform non-enumerating behavior, while the positive test supplies
the functional projection probe. Readiness does not attest the function body,
manufacture product rows, or mutate production state.

## Migration behavior

Migration 021 preflights the 11 data relations, the topology identity relation,
and the existing reader-topology function before creating the read function. It
then strips all non-owner function grants, transfers the new function to the
common owner, and verifies the complete catalog contract. Invalid prerequisites
or hostile metadata abort the migration atomically without leaving the function
or ACL residue.

Migration must execute with `current_user` equal to the common object owner,
which must have effective `CREATE` on `public`. A separate migrator therefore
uses a reviewed temporary `SET ROLE` path and relinquishes that membership
before readiness.

The migration fixes deparser-sensitive session settings transaction-locally and
does not leak them to the migration runner. Production grants are always
reapplied after migration and verified by readiness.

## Acceptance matrix

- A restricted reader role can load the exact approval preview and product
  status for a valid actor, scope, session, authority observation, and
  promotion.
- Wrong promotion, tenant, installation, guild, principal, acting Discord user,
  session digest, or authority kind returns no row without revealing which
  predicate failed.
- Direct `SELECT`, DML, DDL, temporary objects, schema creation, unrelated
  functions, role membership, and grant option are denied.
- Missing reader execution is `CapabilityMissing`; an extra public protected
  function or relation/column grant is `ExcessCapability`.
- Owner, RLS, result shape, language, volatility, strictness, security mode,
  search path, row estimate, logical database, and repeated-role drift fail
  closed.
- Effective grants inherited from hostile defaults and named function grants
  are stripped from both reader functions by migration.
- Split relation ownership aborts migration atomically.
- Existing approval, exact replay, Apply, authority-history, and deployment
  status semantics remain green.
- Package, dependency, comment, workspace, Clippy, format, and real PostgreSQL
  gates remain green.

## Remaining production work

After this slice the decision reader and approval executor are function-scoped,
but Apply still uses direct SQL and broader capabilities. Product ingress stays
closed until Apply and remaining status/rejection paths are scoped, legacy
relation and column grants are atomically sealed, and the final process-wide
schema, object, executable, topology, keyring, and functional readiness gates
all pass together.

Readiness verifies catalog metadata rather than a function-body digest. Body
integrity therefore depends on migration checksums, the restricted `NOLOGIN`
owner, audited DDL credentials, and schema-change evidence. Before claiming a
commercial read SLO, benchmark the function plan on production-shaped data and
record pool-acquire plus end-to-end p50, p95, p99, timeout, and saturation
metrics. Labels stay finite and exclude tenant, promotion, principal, Discord
user, and digest values.
