# Production Control Plane Cutover Runbook

## Status

This runbook describes the fail-closed migration and maintenance contract for
the production control plane. It does not authorize production cutover until
the database-role, RLS, capability-probe, HTTP composition, and
runtime Live gates in the accepted design are implemented and green.

## Required operators and credentials

- `starring_migrator` performs schema migration and ownership handoff.
- `starring_api`, `starring_runtime`, and `starring_maintenance` remain stopped
  until their startup capability probes succeed.
- `starring_owner` is `NOLOGIN` and is never used by an application process.
- Migration, API, runtime, and maintenance credentials are separate secret
  references. They are never passed as command-line literals or committed.

## Preflight

1. Record the running application revision and migration version.
2. Take and verify a restorable PostgreSQL backup.
3. Stop new promotion, approval, rejection, and apply requests.
4. Drain legacy writers, including every old `interaction-smoke` process, and
   confirm no activation is `applying`.
5. Confirm every product-authored promotion is provisioned into exactly one
   active tenant installation with the same tenant, guild, and RuleSet key.
6. Estimate table and index size and schedule a maintenance window for the
   table locks, synchronous index builds, and artifact rewrite in migrations
   004, 006, 007, 012, and 013.
7. Run all migration preflight queries from a read-only transaction and save
   only aggregate counts.

```sql
SELECT pg_catalog.count(*) AS unprovisioned_promotions
FROM public.authoring_promotions AS promotion
LEFT JOIN public.automation_installations AS installation
    ON installation.tenant_id = promotion.tenant_id
    AND installation.installation_id
        = promotion.record #>> '{intent,authority,installation_id}'
    AND installation.discord_guild_id
        = promotion.record #>> '{intent,authority,guild_id}'
    AND installation.ruleset_key
        = promotion.record #>> '{intent,authority,ruleset_key}'
WHERE installation.installation_id IS NULL;

SELECT pg_catalog.count(*) AS applying_activations
FROM public.activation_requests
WHERE state = 'applying';

SELECT pg_catalog.count(*) AS incomplete_product_links
FROM public.activation_requests
WHERE authority_kind = 'product_authoring'
    AND (
        promotion_id IS NULL
        OR promotion_request_digest IS NULL
        OR approval_payload_digest IS NULL
        OR approval_context_digest IS NULL
        OR link_state_name <> 'linked'
    );

SELECT pg_catalog.count(*) AS product_slots_with_legacy_applying
FROM public.activation_requests AS activation
INNER JOIN public.automation_installations AS installation
    ON installation.discord_guild_id = activation.guild_id
    AND installation.ruleset_key = activation.ruleset_key
WHERE activation.authority_kind = 'legacy_manual'
    AND activation.state = 'applying';

WITH ranked_deployments AS (
    SELECT
        deployment.*,
        pg_catalog.row_number() OVER (
            PARTITION BY deployment.tenant_id,
                deployment.installation_id,
                deployment.guild_id,
                deployment.ruleset_key
            ORDER BY deployment.runtime_generation DESC,
                deployment.deployment_id DESC
        ) AS generation_rank
    FROM public.runtime_deployments AS deployment
)
SELECT pg_catalog.count(*) AS product_pointer_lineage_failures
FROM public.automation_installations AS installation
INNER JOIN public.automation_ruleset_activations AS active
    ON active.guild_id = installation.discord_guild_id
    AND active.ruleset_key = installation.ruleset_key
LEFT JOIN ranked_deployments AS deployment
    ON deployment.tenant_id = installation.tenant_id
    AND deployment.installation_id = installation.installation_id
    AND deployment.guild_id = installation.discord_guild_id
    AND deployment.ruleset_key = installation.ruleset_key
    AND deployment.target_version = active.active_version
    AND deployment.generation_rank = 1
LEFT JOIN public.activation_requests AS activation
    ON activation.id = deployment.activation_request_id
LEFT JOIN public.automation_ruleset_versions AS version
    ON version.guild_id = deployment.guild_id
    AND version.ruleset_key = deployment.ruleset_key
    AND version.version = deployment.target_version
WHERE deployment.deployment_id IS NULL
    OR activation.authority_kind IS DISTINCT FROM 'product_authoring'
    OR activation.link_state_name IS DISTINCT FROM 'linked'
    OR activation.state IS DISTINCT FROM 'applied'
    OR activation.tenant_id IS DISTINCT FROM deployment.tenant_id
    OR activation.installation_id IS DISTINCT FROM deployment.installation_id
    OR activation.promotion_id IS DISTINCT FROM deployment.promotion_id
    OR activation.guild_id IS DISTINCT FROM deployment.guild_id
    OR activation.ruleset_key IS DISTINCT FROM deployment.ruleset_key
    OR activation.target_version IS DISTINCT FROM deployment.target_version
    OR activation.target_content_hash IS DISTINCT FROM deployment.target_content_hash
    OR version.content_hash IS DISTINCT FROM deployment.target_content_hash;

SELECT
    pg_catalog.count(*) AS ruleset_artifact_rows,
    pg_catalog.pg_total_relation_size(
        'public.automation_ruleset_versions'::REGCLASS
    ) AS ruleset_artifact_total_bytes,
    pg_catalog.max(pg_catalog.octet_length(definition::TEXT))
        AS largest_ruleset_definition_bytes,
    pg_catalog.count(*) FILTER (
        WHERE schema_version NOT BETWEEN 1 AND 4294967295
            OR pg_catalog.jsonb_typeof(definition) <> 'object'
            OR pg_catalog.octet_length(definition::TEXT) > 524288
    ) AS ruleset_artifact_shape_failures
FROM public.automation_ruleset_versions;

WITH shadow_targets(source, guild_id, ruleset_key, target_version, target_hash) AS (
    SELECT 'activation', guild_id, ruleset_key, target_version,
        target_content_hash
    FROM public.activation_requests
    UNION ALL
    SELECT 'deployment', guild_id, ruleset_key, target_version,
        target_content_hash
    FROM public.runtime_deployments
    UNION ALL
    SELECT 'attestation', guild_id, ruleset_key, target_version,
        target_content_hash
    FROM public.runtime_attestations
    UNION ALL
    SELECT 'serving', guild_id, ruleset_key, target_version,
        target_content_hash
    FROM public.runtime_serving_leases
)
SELECT shadow.source, pg_catalog.count(*) AS mismatches
FROM shadow_targets AS shadow
LEFT JOIN public.automation_ruleset_versions AS version
    ON version.guild_id = shadow.guild_id
    AND version.ruleset_key = shadow.ruleset_key
    AND version.version = shadow.target_version
WHERE version.guild_id IS NULL
    OR version.content_hash IS DISTINCT FROM shadow.target_hash
GROUP BY shadow.source
ORDER BY shadow.source;
```

Every control-plane failure count, `ruleset_artifact_shape_failures`, and every
returned shadow mismatch count must be zero. Record the artifact row, table-size,
and largest-definition values for the migration rehearsal. A nonzero failure
count stops the cutover; do not weaken or skip the migration constraints.

## Migration sequence

1. Keep API and runtime processes stopped.
2. Apply all pending migrations with the migrator credential.
3. Do not retry a failed migration blindly. Capture SQLSTATE and the stable
   constraint message, repair the preflight data through an audited operator
   path, then restart from a fresh transaction.
4. Run schema, function-signature, ownership, grant, default-privilege, RLS,
   and direct-DML denial probes.
5. Start only the API readiness process. It must verify the configured approval
   HMAC keyring covers all live approval receipts.
6. Start maintenance and runtime readiness processes separately.
7. Re-enable ingress only after every least-privilege probe is green.

Migration 004 deliberately takes strong locks and fails when legacy promotions
cannot be scoped to provisioned installations. Migrations 006 and 007 build
bounded retention indexes synchronously. Migration 007 is forward-only after
its first successful receipt purge because live replay receipts may no longer
exist; rollback then requires backup restore or a forward fix.

Migration 012 adds and materializes a stored canonical RuleSet hash for every
published artifact, validates the full artifact table, and checks every
activation and runtime hash shadow. It therefore requires an exclusive-write
maintenance window sized from a production-like rehearsal. Set a bounded
`lock_timeout` for lock acquisition and a rehearsed, bounded
`statement_timeout` for the rewrite and validation; an expiry aborts the whole
migration transaction. Do not start API, authoring, or runtime writers between
the rewrite and the post-migration capability probes.

Migration 012 proves current content against its stored hash and any retained
activation or runtime hash shadow. A legacy artifact whose definition and hash
were both altered before migration and which has no retained shadow has no
independent database trust anchor. Restore such history only from a verified
backup or signed external evidence; never declare it trusted from a newly
computed self-hash alone.

Migration 012 also revokes public execution of the canonical hash functions.
The ownership-and-grant migration must explicitly give only the approved
RuleSet publishing boundary the minimum execution capability needed by the
stored generated expression. Before ingress opens, a non-owner publish probe
must succeed through that boundary while direct table mutation and direct
function execution from API and runtime roles remain denied.

Migration 013 takes strong locks over `automation_installations`,
`activation_requests`, `automation_ruleset_activations`, `runtime_deployments`,
and `automation_ruleset_versions`. Its preflight rejects product `Applying`
residue, in-flight legacy activation in a product slot, and any product pointer
without exact latest-deployment lineage. Legacy or generic direct activation
and product installation takeover serialize through the same
transaction-scoped slot advisory lock. Product Apply instead retains its
product-lane lock and atomic transaction. Deferred invariants re-read the final
transaction state, so a product pointer may change only with exact
latest-deployment lineage. Do not bypass its triggers, disable trigger
execution, or grant application roles direct execution on its security-definer
functions.

Migration 014 creates
`public.starring_product_installation_authority_read_v1(TEXT,TEXT,BYTEA)` as
the only supported installation-authority read boundary. It is volatile,
strict, parallel-unsafe, security-definer, and fixed to
`search_path=pg_catalog`. The migration fails with SQLSTATE `55000` unless the
five referenced identity, tenant, installation, and authority relations exist
under one owner. It transfers the function to that owner and revokes `PUBLIC`
execution in the same transaction. The owner must be the non-login,
non-superuser, non-`BYPASSRLS` `starring_owner` role before production
readiness is attempted. Migration 014 also removes every default-privilege
function grant inherited by a non-owner role before transferring ownership.
Revoke temporary migrator-to-owner membership after the ownership handoff; the
installation-authority API readiness contract rejects memberships into or out
of the owner role.

If `starring_owner` does not own the `public` schema, the role bootstrap must
grant it schema usage so the security-definer body can resolve its fully
qualified relations after `PUBLIC` privileges are revoked. It must grant
`starring_api` schema usage and execution of only the exact versioned signature
without grant option:

```sql
GRANT USAGE ON SCHEMA public TO starring_owner;
GRANT USAGE ON SCHEMA public TO starring_api;
GRANT EXECUTE ON FUNCTION
    public.starring_product_installation_authority_read_v1(TEXT, TEXT, BYTEA)
TO starring_api;
```

For this slice, `starring_api` must have no `SELECT`, `INSERT`, `UPDATE`,
`DELETE`, `TRUNCATE`, `REFERENCES`, or `TRIGGER` privilege on
`product_principals`, `product_auth_sessions`, `product_tenants`,
`automation_installations`, or
`automation_installation_authority_versions`. It must also lack database
`CREATE` and `TEMPORARY`, schema `CREATE`, owner membership, superuser,
`CREATEDB`, `CREATEROLE`, replication, and `BYPASSRLS`. Call
`PostgresInstallationAuthoritySource::verify_readiness` before opening ingress;
it checks the exact function result contract, owner and ACL, current-role
capabilities, a direct login session, absence of all role memberships, table-
and column-level privilege denial, and executes a data-independent empty-scope
probe under a bounded read-only transaction. Running readiness after `SET ROLE`
is rejected because that session can reset to the more privileged login role.
The execution probe also fails closed when the function owner lacks schema
usage.

This authority-read probe certifies only that adapter boundary. Authentication
and authorized-snapshot access still need their own narrow database functions
before one final execute-only `starring_api` process can pass whole-service
readiness. Do not compensate by granting the API role direct table access.

`interaction-smoke` is test-only manual tooling, not an operational fallback.
It requires the `legacy-smoke` compile feature,
`STARRING_ALLOW_INTERACTION_SMOKE=1`, is marked non-publishable, and requires an
ASCII alphanumeric/underscore database name with the `starring_` prefix and an
underscore-delimited `test` segment. These controls do not authenticate Discord
credentials. Never pass a production bot token or production guild identity to
it, and confirm every old smoke process is drained before migration or ingress.
Exclude the binary and both smoke features from every production artifact and
deployment manifest; `publish = false` is not a deployment security boundary.

## Identity retention

- Run `starring_purge_product_identity_v1` only through the maintenance adapter.
- Batch size is 1 through 1,000.
- The adapter uses transaction-local statement and lock deadlines.
- Continue bounded calls while `backlog_remaining` is true, with scheduler
  jitter and a process-level concurrency of one per database.
- A timeout or lock conflict is retryable. An invalid result or indeterminate
  commit stops the worker and pages an operator.
- Never set the retention gate or delete identity rows directly.

## Approval receipt retention

- Exact replay is guaranteed through
  `completed_at + interval '7 days'`.
- Purge only through the maintenance adapter and only for
  `product_approve_v1`.
- One call locks at most 1,000 receipts and removes at most 32 aliases per
  receipt before deleting the receipt.
- Audit events and immutable receipt audit evidence remain permanently.
- A delayed purge may extend replay availability but is not an advertised
  guarantee.
- Never delete audit events, audit evidence, receipts, or aliases directly.

## Approval HMAC key rotation

1. Generate a new 32-byte random key in the production secret store with a new
   immutable key ID.
2. Deploy writers with `[new, old]`; new is active and old remains a retired
   verification key.
3. Drain all old-only writers.
4. Wait at least seven days after the last old-only write.
5. Run bounded receipt purge until the eligible backlog is empty.
6. Probe live-receipt coverage with `[new]`.
7. If coverage is incomplete, retain the old key and investigate. Never force
   the probe or reuse a key ID with different material.
8. Deploy `[new]` only after coverage succeeds.
9. Destroy the old secret after rollout overlap and readiness remain green.

The keyring supports at most eight keys. Rotation must complete before that
limit is approached. Receipt evidence deliberately excludes HMAC digests, key
IDs, and fingerprints so archived audit integrity does not extend secret
retention.

## Failure and rollback

- Before any receipt purge, rollback is binary and migration rollback to the
  previously tested revision, followed by capability probes.
- After receipt purge, use a forward fix or restore the verified backup. Do not
  recreate receipts from audit data and do not synthesize aliases without the
  original raw idempotency key.
- If an approval response is lost, retry the same request inside the replay
  window. If the database commit outcome is indeterminate, do not issue a new
  idempotency key until status and receipt probes resolve the outcome.
- If API or runtime capability probes fail, keep ingress closed. Owner
  credentials are not an emergency application fallback.

## Evidence to retain

- application and migration revisions
- aggregate preflight counts
- migration duration and lock-wait metrics
- role capability-probe results
- retention deleted counts and backlog flags
- keyring coverage outcome and key IDs only
- backup and restore-drill identifiers

Do not retain credentials, raw OAuth state, cookies, CSRF values, raw
idempotency keys, RuleSet JSON, or user message bodies in operational evidence.
