# macOS Starring Runtime Staging Operations

This runbook installs and operates `starring-runtime` as the logged-in Mac
mini user's LaunchAgent. The staging service is
`local.starring.runtime.staging`, its health listener is loopback-only at
`127.0.0.1:19091`, and every database URL and the Discord bot token are
resolved indirectly from macOS Keychain.

This is an empty-open runtime substrate. A successful startup can acquire and
renew the production owner, connect Discord in the paused state, publish the
durable ingress acknowledgement, and report ready. It does not install a
customer interaction route, populate the in-process registry, or execute a
customer Discord interaction. Do not describe `ready` as customer traffic
serving until a separately reviewed route-admission release exists.

## Fixed operating contract

| Item | Value |
| --- | --- |
| repository | `/Users/jungbogeon/starring` |
| launchd label | `local.starring.runtime.staging` |
| launchd template | `ops/macos/local.starring.runtime.staging.plist` |
| installed plist | `~/Library/LaunchAgents/local.starring.runtime.staging.plist` |
| binary link | `~/.local/libexec/starring-runtime` |
| health listener | `127.0.0.1:19091` |
| runtime log | `~/Library/Logs/starring-runtime/runtime.log` |
| process shutdown bound | 30 seconds |
| launchd exit timeout | 35 seconds |
| PostgreSQL pools | 5 roles × 2 connections, ceiling 10 |
| role bootstrap | `ops/postgres/staging-runtime-role-bootstrap.sql` |

The runtime requires exactly five distinct PostgreSQL login identities:

| Runtime configuration slot | Default role | Exact capability |
| --- | --- | --- |
| `CONVERGENCE` | `starring_runtime_execution` | execution and recovery functions |
| `EXACT_TARGET` | `starring_runtime_exact_target` | exact target hydration functions |
| `PANEL` | `starring_runtime_panel` | public panel reconciliation functions |
| `SERVING` | `starring_runtime_serving` | serving lease functions |
| `INTERACTION` | `starring_runtime_interaction` | pinned route and instance functions |

The `CONVERGENCE` environment name is retained by the runtime configuration
API, but that pool is deliberately verified against the execution database
contract. Do not reuse any role or database URL between slots. The bootstrap
grants `CONNECT`, `USAGE` on `public`, and only the exact security-definer
functions declared by the migrations. It grants no table, column, sequence,
schema `CREATE`, role membership, or foreign-database capability.

## Independently approved target contract

Operate from the same logged-in macOS account that owns the LaunchAgent and
unlocked login Keychain. Do not run the runtime under `sudo`.

The target is fixed to database `starring_runtime_staging` on
`127.0.0.1:5432`. Before using this runbook, export these three non-secret
values from a separately reviewed inventory or change record into the operator
shell:

- `STARRING_STAGING_CLUSTER_ADMIN`
- `STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER`
- `STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT`

Do not derive the expected system identifier by querying the target during
this procedure. The acknowledgement must equal this exact value, with the
reviewed identifier substituted:

```text
starring-runtime-dedicated-staging-cluster-v2:SYSTEM_IDENTIFIER:starring_runtime_staging:cluster-wide-public-acl-reset:bidirectional-runtime-membership-revocation
```

Every `zsh` block below is a fail-fast subshell. Run a block as one unit and
continue only when its exit status is zero. Secrets are entered only at an
interactive password prompt.

## Local preconditions and independent inventory

```zsh
(
  set -euo pipefail
  cd /Users/jungbogeon/starring
  STAGING_DATABASE=starring_runtime_staging
  : "${STARRING_STAGING_CLUSTER_ADMIN:?load the reviewed cluster administrator}"
  : "${STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER:?load the reviewed system identifier}"
  : "${STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT:?load the reviewed dedicated-cluster acknowledgement}"
  EXPECTED_ACKNOWLEDGEMENT="starring-runtime-dedicated-staging-cluster-v2:${STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER}:${STAGING_DATABASE}:cluster-wide-public-acl-reset:bidirectional-runtime-membership-revocation"
  test "$STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT" = "$EXPECTED_ACKNOWLEDGEMENT"
  print -r -- "$STARRING_STAGING_CLUSTER_ADMIN" \
    | grep -Eq '^[a-z_][a-z0-9_]{0,62}$'
  print -r -- "$STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER" \
    | grep -Eq '^[0-9]+$'
  test -z "$(git status --porcelain --untracked-files=normal)"
  test "$(git rev-parse --is-inside-work-tree)" = true
  test -x /opt/homebrew/opt/postgresql@16/bin/psql
  test -x /usr/bin/security
  test -x /usr/bin/perl
  plutil -lint ops/macos/local.starring.runtime.staging.plist
  AVAILABLE_KIB="$(df -Pk /Users/jungbogeon | awk 'NR == 2 { print $4 }')"
  test "$AVAILABLE_KIB" -ge 31457280
)
```

The disk check enforces at least 30 GiB free on the filesystem containing the
repository. Stop before building, migrating, or rotating credentials if it
fails.

The PostgreSQL role script intentionally removes `PUBLIC` database privileges
cluster-wide so the five runtime roles cannot connect to another database
through inherited `PUBLIC` access. A staging-looking database name is not
proof that its cluster is dedicated. The independently reviewed system
identifier and exact cluster-wide ACL acknowledgement are both mandatory.
The acknowledgement also approves removal of every inbound and outbound role
membership involving a runtime capability role. Each removal uses the original
grantor and `CASCADE`, so dependent role grants can also be removed. This
bootstrap is restricted to PostgreSQL 16 because its system-object `PUBLIC`
baseline is checked against PostgreSQL 16 catalog metadata.

## Authenticate and prove the exact target before any migration

This read-only proof is the first database connection in the procedure. It
uses an explicit TCP host, port, database, and independently inventoried
administrator. The observed database name, PostgreSQL system identifier,
current user, session user, superuser status, and PostgreSQL major version must
match one exact expected record. Do not apply a migration, run the role
bootstrap, or change a credential if this proof fails. Never replace the
inventory value with an identifier learned from this connection.

```zsh
(
  set -euo pipefail
  STAGING_DATABASE=starring_runtime_staging
  STAGING_DATABASE_HOST=127.0.0.1
  STAGING_DATABASE_PORT=5432
  : "${STARRING_STAGING_CLUSTER_ADMIN:?load the reviewed cluster administrator}"
  : "${STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER:?load the reviewed system identifier}"
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  EXPECTED_TARGET="${STAGING_DATABASE}|${STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER}|${STARRING_STAGING_CLUSTER_ADMIN}|${STARRING_STAGING_CLUSTER_ADMIN}|true|16"
  OBSERVED_TARGET="$(
    PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
      --no-psqlrc --set ON_ERROR_STOP=1 --password --quiet \
      --host "$STAGING_DATABASE_HOST" --port "$STAGING_DATABASE_PORT" \
      --username "$STARRING_STAGING_CLUSTER_ADMIN" \
      --dbname "$STAGING_DATABASE" --tuples-only --no-align \
      --command "BEGIN READ ONLY" \
      --command "SELECT pg_catalog.concat_ws('|', pg_catalog.current_database(), control.system_identifier::TEXT, current_user, session_user, administrator.rolsuper::TEXT, (pg_catalog.current_setting('server_version_num')::INTEGER / 10000)::TEXT) FROM pg_catalog.pg_control_system() AS control CROSS JOIN pg_catalog.pg_roles AS administrator WHERE administrator.rolname = current_user" \
      --command "COMMIT"
  )"
  test "$OBSERVED_TARGET" = "$EXPECTED_TARGET"
)
```

## Quiesce the dedicated cluster before database work

Bootstrap, migration, and credential rotation require both the runtime and API
LaunchAgents to be unloaded. Stop every maintenance job, migration process,
connection pool, and ad-hoc SQL client that can reach any database in this
dedicated cluster. The SQL guard rejects every other cluster-wide client
backend, every non-client backend authenticated as one of the five runtime
roles, and every prepared transaction, regardless of database or session
identity.

```zsh
(
  set -euo pipefail
  DOMAIN="gui/$(id -u)"
  API_SERVICE="$DOMAIN/local.starring.api.staging"
  API_WAS_LOADED=false
  SERVICES=(
    "$DOMAIN/local.starring.runtime.staging"
    "$API_SERVICE"
  )
  for SERVICE in "${SERVICES[@]}"; do
    if SERVICE_STATE="$(launchctl print "$SERVICE" 2>/dev/null)"; then
      if test "$SERVICE" = "$API_SERVICE"; then
        API_WAS_LOADED=true
      fi
      PID="$(
        print -r -- "$SERVICE_STATE" \
          | awk '/^[[:space:]]*pid = / { print $3; exit }'
      )"
      launchctl bootout "$SERVICE"
      if test -n "$PID"; then
        for ATTEMPT in {1..35}; do
          if ! kill -0 "$PID" 2>/dev/null; then
            break
          fi
          sleep 1
        done
        ! kill -0 "$PID" 2>/dev/null
      fi
    fi
    ! launchctl print "$SERVICE" >/dev/null 2>&1
  done
  print -r -- "api_was_loaded=$API_WAS_LOADED"
)
```

Preserve the printed `api_was_loaded` value in the change record. A previously
loaded API must remain down during this procedure and must be restored by the
conditional recovery section only after all runtime acceptance checks pass.

## Verify the exact SQLx migration ledger

Apply every repository migration to the target staging database as the common
schema owner before continuing. This check compares the exact ordered set of
repository migration versions and SQLx SHA-384 checksums with
`public._sqlx_migrations`; a missing, extra, failed, or modified migration
fails the block.

```zsh
(
  set -euo pipefail
  cd /Users/jungbogeon/starring
  STAGING_DATABASE=starring_runtime_staging
  STAGING_DATABASE_HOST=127.0.0.1
  STAGING_DATABASE_PORT=5432
  : "${STARRING_STAGING_CLUSTER_ADMIN:?load the reviewed cluster administrator}"
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  EXPECTED_LEDGER="$(mktemp)"
  APPLIED_LEDGER="$(mktemp)"
  trap 'rm -f -- "$EXPECTED_LEDGER" "$APPLIED_LEDGER"' EXIT
  for MIGRATION in migrations/*.sql; do
    BASENAME="$(basename "$MIGRATION")"
    print -r -- "$BASENAME" | grep -Eq '^[0-9]+_.+\.sql$'
    VERSION="${BASENAME%%_*}"
    CHECKSUM="$(shasum -a 384 "$MIGRATION" | awk '{ print $1 }')"
    print -r -- "$VERSION:$CHECKSUM"
  done | LC_ALL=C sort -t: -k1,1n >"$EXPECTED_LEDGER"
  test "$(
    cut -d: -f1 "$EXPECTED_LEDGER" | uniq -d | wc -l | tr -d ' '
  )" = 0
  PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
    --no-psqlrc --set ON_ERROR_STOP=1 --password \
    --host "$STAGING_DATABASE_HOST" --port "$STAGING_DATABASE_PORT" \
    --username "$STARRING_STAGING_CLUSTER_ADMIN" \
    --dbname "$STAGING_DATABASE" --tuples-only --no-align \
    --command "SELECT version::TEXT || ':' || CASE WHEN success THEN pg_catalog.encode(checksum, 'hex') ELSE 'failed' END FROM public._sqlx_migrations ORDER BY version" \
    >"$APPLIED_LEDGER"
  diff -u "$EXPECTED_LEDGER" "$APPLIED_LEDGER"
)
```

## Bootstrap the five database roles

Run the script in quarantine mode. A session advisory lock serializes valid
invocations for this database and cluster identity. The first transaction
validates PostgreSQL 16, the exact target identity, administrator authority,
the exact five fixed capability-role names, and the dedicated-cluster
acknowledgement. Role-name overrides are forbidden. The second transaction
only creates or alters those five roles, strips privileged attributes, clears
their passwords, and commits them as `NOLOGIN`. The third transaction removes
every inbound and outbound membership involving those roles under each
original grantor with `CASCADE`, then commits. The fourth transaction rejects
external ownership without changing it, proves cluster-wide zero client
backends and zero prepared transactions, resets settings, removes every direct
target-role ACL and grant option across user and system objects, establishes
the exact capability grants, verifies the result, repeats the cluster isolation
proof, and only then commits.

If membership removal fails, the credential seal remains committed and the
membership transaction rolls back. If ownership, cluster quiescence, ACL
validation, or configuration later fails, both the credential seal and
completed membership removals remain committed. Do not treat that state as a
successful bootstrap: stop all clients, correct the reported drift, and
complete quarantine mode before setting passwords.

The default-privilege mutation is deliberately narrow. Direct grants to the
five runtime roles are removed from every current-database default ACL.
`PUBLIC` table, sequence, routine, type, and schema defaults are suppressed
only for owners that can currently create in the runtime-accessible `public`
schema. `PUBLIC` privileges are removed from every database and from schema
`public`. PostgreSQL 16 built-in system-schema, relation, column, routine, type,
and language baselines remain unchanged; added `PUBLIC` privileges are
detected from `pg_init_privs` and fail closed instead of being rewritten. Every
manifest capability function must have exactly two non-grantable `EXECUTE`
entries: its owner and its one designated capability role. Quarantine mode
removes `PUBLIC` and unrelated-role grants from those manifest functions.

```zsh
(
  set -euo pipefail
  cd /Users/jungbogeon/starring
  STAGING_DATABASE=starring_runtime_staging
  STAGING_DATABASE_HOST=127.0.0.1
  STAGING_DATABASE_PORT=5432
  DOMAIN="gui/$(id -u)"
  SERVICE="$DOMAIN/local.starring.runtime.staging"
  : "${STARRING_STAGING_CLUSTER_ADMIN:?load the reviewed cluster administrator}"
  : "${STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER:?load the reviewed system identifier}"
  : "${STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT:?load the reviewed dedicated-cluster acknowledgement}"
  ! launchctl print "$SERVICE" >/dev/null 2>&1
  ! launchctl print "$DOMAIN/local.starring.api.staging" >/dev/null 2>&1
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
    --no-psqlrc --set ON_ERROR_STOP=1 --password \
    --host "$STAGING_DATABASE_HOST" --port "$STAGING_DATABASE_PORT" \
    --username "$STARRING_STAGING_CLUSTER_ADMIN" \
    --set runtime_enable=off \
    --set expected_database="$STAGING_DATABASE" \
    --set expected_system_identifier="$STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER" \
    --set runtime_dedicated_cluster_acknowledgement="$STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT" \
    --dbname "$STAGING_DATABASE" \
    --command "SELECT 1 / CASE WHEN NOT EXISTS (SELECT 1 FROM pg_catalog.pg_stat_activity WHERE pid <> pg_catalog.pg_backend_pid() AND (backend_type = 'client backend' OR usesysid IN (pg_catalog.to_regrole('starring_runtime_execution'), pg_catalog.to_regrole('starring_runtime_exact_target'), pg_catalog.to_regrole('starring_runtime_panel'), pg_catalog.to_regrole('starring_runtime_serving'), pg_catalog.to_regrole('starring_runtime_interaction')))) AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_prepared_xacts) THEN 1 ELSE 0 END AS cluster_quiescence_proof" \
    --file ops/postgres/staging-runtime-role-bootstrap.sql
)
```

Bootstrap mode is deliberately fail-closed. It commits the five roles as
`NOLOGIN` with null passwords before attempting membership cleanup, then
commits bidirectional membership removal before checking ownership, cluster
quiescence, and ACLs. A client that raced the proof, a prepared transaction,
an ACL lock, an owned object, or invalid `PUBLIC` state can therefore fail the
last transaction after the seal and membership changes are durable. Stop every
cluster client, release the lock or prepared transaction, separately review
owned objects and system `PUBLIC` drift, and rerun quarantine mode. The
bootstrap never drops or reassigns an owned object. Do not set passwords until
the entire block succeeds.

Generate a separate password for every role in a password manager. Each
password must contain 24–512 characters from only `A-Z`, `a-z`, `0-9`, `_`,
`-`, `.`, and `~`; 32 or more random characters is the operational minimum.
Use the client-side `\password` command so plaintext is not placed in SQL,
server statement logs, process arguments, or shell history:

```zsh
(
  set -euo pipefail
  STAGING_DATABASE=starring_runtime_staging
  : "${STARRING_STAGING_CLUSTER_ADMIN:?load the reviewed cluster administrator}"
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  exec env PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
    --no-psqlrc --set ON_ERROR_STOP=1 --password \
    --host 127.0.0.1 --port 5432 \
    --username "$STARRING_STAGING_CLUSTER_ADMIN" \
    --dbname "$STAGING_DATABASE"
)
```

```text
\password starring_runtime_execution
\password starring_runtime_exact_target
\password starring_runtime_panel
\password starring_runtime_serving
\password starring_runtime_interaction
\q
```

## Install the fail-closed PostgreSQL HBA boundary

Database ACLs cannot protect a database created after this bootstrap because a
future PostgreSQL database can begin with `PUBLIC CONNECT` and `TEMPORARY`.
Before enabling the roles, use separately reviewed configuration management to
prepend the following block as the first four effective rules in the
`pg_hba.conf` reported by `SHOW hba_file`. Do not append it after a broader
matching rule and do not weaken `scram-sha-256`.

```text
hostnossl starring_runtime_staging starring_runtime_execution,starring_runtime_exact_target,starring_runtime_panel,starring_runtime_serving,starring_runtime_interaction 127.0.0.1/32 scram-sha-256
host all starring_runtime_execution,starring_runtime_exact_target,starring_runtime_panel,starring_runtime_serving,starring_runtime_interaction 0.0.0.0/0 reject
host all starring_runtime_execution,starring_runtime_exact_target,starring_runtime_panel,starring_runtime_serving,starring_runtime_interaction ::0/0 reject
local all starring_runtime_execution,starring_runtime_exact_target,starring_runtime_panel,starring_runtime_serving,starring_runtime_interaction reject
```

The first rule permits only the exact five roles, exact staging database,
unencrypted loopback transport required by the runtime URL contract, and SCRAM
authentication. The next three rules reject every other IPv4, IPv6, and Unix
socket database connection for those roles before any pre-existing general
rule can match.

Print the authoritative path without editing it through this runbook:

```zsh
(
  set -euo pipefail
  : "${STARRING_STAGING_CLUSTER_ADMIN:?load the reviewed cluster administrator}"
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  HBA_FILE="$(
    PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
      --no-psqlrc --set ON_ERROR_STOP=1 --password \
      --host 127.0.0.1 --port 5432 \
      --username "$STARRING_STAGING_CLUSTER_ADMIN" \
      --dbname starring_runtime_staging --tuples-only --no-align \
      --command 'SHOW hba_file'
  )"
  test -n "$HBA_FILE"
  print -r -- "$HBA_FILE"
)
```

After the reviewed edit, require the parsed rules to have no errors and the
managed block to be exactly effective rules 1–4. Reload it, wait one second,
and repeat the parsed proof through the same authenticated administrator
connection:

```zsh
(
  set -euo pipefail
  : "${STARRING_STAGING_CLUSTER_ADMIN:?load the reviewed cluster administrator}"
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  HBA_PROOF_QUERY="
    WITH expected AS (
      SELECT ARRAY[
        'starring_runtime_execution',
        'starring_runtime_exact_target',
        'starring_runtime_panel',
        'starring_runtime_serving',
        'starring_runtime_interaction'
      ]::TEXT[] AS users
    ),
    managed AS (
      SELECT rule.*
      FROM pg_catalog.pg_hba_file_rules AS rule
      CROSS JOIN expected
      WHERE rule.user_name = expected.users
    )
    SELECT pg_catalog.concat_ws(
      '|',
      (
        SELECT pg_catalog.count(*)
        FROM pg_catalog.pg_hba_file_rules
        WHERE error IS NOT NULL
      ),
      pg_catalog.count(*),
      pg_catalog.count(*) FILTER (
        WHERE rule_number = 1
          AND type = 'hostnossl'
          AND database = ARRAY['starring_runtime_staging']::TEXT[]
          AND address = '127.0.0.1'
          AND netmask = '255.255.255.255'
          AND auth_method = 'scram-sha-256'
      ),
      pg_catalog.count(*) FILTER (
        WHERE rule_number = 2
          AND type = 'host'
          AND database = ARRAY['all']::TEXT[]
          AND address = '0.0.0.0'
          AND netmask = '0.0.0.0'
          AND auth_method = 'reject'
      ),
      pg_catalog.count(*) FILTER (
        WHERE rule_number = 3
          AND type = 'host'
          AND database = ARRAY['all']::TEXT[]
          AND address = '::'
          AND netmask = '::'
          AND auth_method = 'reject'
      ),
      pg_catalog.count(*) FILTER (
        WHERE rule_number = 4
          AND type = 'local'
          AND database = ARRAY['all']::TEXT[]
          AND address IS NULL
          AND netmask IS NULL
          AND auth_method = 'reject'
      )
    )
    FROM managed
  "
  HBA_VALIDATION="$(
    PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
      --no-psqlrc --set ON_ERROR_STOP=1 --password \
      --host 127.0.0.1 --port 5432 \
      --username "$STARRING_STAGING_CLUSTER_ADMIN" \
      --dbname starring_runtime_staging --tuples-only --no-align \
      --command "$HBA_PROOF_QUERY" \
      --command "SELECT CASE WHEN pg_catalog.pg_reload_conf() THEN 'reloaded' ELSE 'reload_failed' END" \
      --command 'SELECT pg_catalog.pg_sleep(1)' \
      --command "$HBA_PROOF_QUERY" \
      | sed '/^$/d'
  )"
  test "$(
    print -r -- "$HBA_VALIDATION" \
      | grep -Fxc '0|4|1|1|1|1'
  )" = 2
  test "$(
    print -r -- "$HBA_VALIDATION" \
      | grep -Fxc 'reloaded'
  )" = 1
)
```

Run the same script in enable mode. It requires five SCRAM-SHA-256 verifiers,
requires each role to be committed `NOLOGIN`, removes any newly introduced
bidirectional membership, verifies zero client sessions and prepared
transactions across the dedicated cluster plus the exact ACL boundary, repeats
the isolation proof, and only then changes all five roles to `LOGIN` in one
final transaction. Any failure rolls that activation transaction back to the
committed `NOLOGIN` state.

```zsh
(
  set -euo pipefail
  cd /Users/jungbogeon/starring
  STAGING_DATABASE=starring_runtime_staging
  : "${STARRING_STAGING_CLUSTER_ADMIN:?load the reviewed cluster administrator}"
  : "${STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER:?load the reviewed system identifier}"
  : "${STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT:?load the reviewed dedicated-cluster acknowledgement}"
  DOMAIN="gui/$(id -u)"
  ! launchctl print "$DOMAIN/local.starring.runtime.staging" >/dev/null 2>&1
  ! launchctl print "$DOMAIN/local.starring.api.staging" >/dev/null 2>&1
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
    --no-psqlrc --set ON_ERROR_STOP=1 --password \
    --host 127.0.0.1 --port 5432 \
    --username "$STARRING_STAGING_CLUSTER_ADMIN" \
    --set runtime_enable=on \
    --set expected_database="$STAGING_DATABASE" \
    --set expected_system_identifier="$STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER" \
    --set runtime_dedicated_cluster_acknowledgement="$STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT" \
    --dbname "$STAGING_DATABASE" \
    --command "SELECT 1 / CASE WHEN NOT EXISTS (SELECT 1 FROM pg_catalog.pg_stat_activity WHERE pid <> pg_catalog.pg_backend_pid() AND (backend_type = 'client backend' OR usesysid IN (pg_catalog.to_regrole('starring_runtime_execution'), pg_catalog.to_regrole('starring_runtime_exact_target'), pg_catalog.to_regrole('starring_runtime_panel'), pg_catalog.to_regrole('starring_runtime_serving'), pg_catalog.to_regrole('starring_runtime_interaction')))) AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_prepared_xacts) THEN 1 ELSE 0 END AS cluster_quiescence_proof" \
    --file ops/postgres/staging-runtime-role-bootstrap.sql
)
```

Any quarantine-mode rerun intentionally returns all five roles to `NOLOGIN`,
clears their passwords, and requires a fresh password-and-enable cycle. Enable
mode refuses roles that are already `LOGIN`; it is an activation edge, not an
idempotent health check. Stop and unload both staging LaunchAgents first. This is
credential rotation, not a harmless read-only check.

From the moment enable mode succeeds until every authentication, HBA,
cross-database, capability-readiness, future-database, and service-readiness
proof in this runbook succeeds, any nonzero block exit or unexpected output is
an activation failure. Keep or make the LaunchAgent unloaded and immediately
rerun the earlier quarantine-mode bootstrap block before inspecting logs,
editing HBA, retrying a probe, or cleaning up a probe database. That mandatory
reseal commits all five roles as `NOLOGIN` with null passwords even when later
ACL work fails. Do not leave the roles `LOGIN` under an unproven boundary.

## Prove authentication and database isolation

The SCRAM verifier alone does not prove that the active HBA requires a
password. For each role, the first target probe uses a deliberately invalid
password and must reach the SCRAM rule, the second prompts for the correct
password and invokes that role's readiness function, and the third prompts
again and must be rejected by HBA on `postgres`.

```zsh
(
  set -euo pipefail
  STAGING_DATABASE=starring_runtime_staging
  ROLES=(
    starring_runtime_execution
    starring_runtime_exact_target
    starring_runtime_panel
    starring_runtime_serving
    starring_runtime_interaction
  )
  READINESS_FUNCTIONS=(
    public.starring_runtime_execution_database_readiness_v1
    public.starring_runtime_exact_target_database_readiness_v1
    public.starring_runtime_panel_database_readiness_v1
    public.starring_runtime_serving_database_readiness_v1
    public.starring_runtime_interaction_database_readiness_v1
  )
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  TARGET_ERROR="$(mktemp)"
  POSTGRES_ERROR="$(mktemp)"
  trap 'rm -f -- "$TARGET_ERROR" "$POSTGRES_ERROR"' EXIT
  for INDEX in {1..5}; do
    ROLE="${ROLES[$INDEX]}"
    READINESS_FUNCTION="${READINESS_FUNCTIONS[$INDEX]}"
    : >"$TARGET_ERROR"
    if LC_ALL=C PGSSLMODE=disable PGPASSWORD='invalid password probe' \
      /opt/homebrew/opt/postgresql@16/bin/psql \
        --no-psqlrc --set ON_ERROR_STOP=1 \
        --host 127.0.0.1 --port 5432 --username "$ROLE" \
        --dbname "$STAGING_DATABASE" \
        --command 'SELECT 1' >/dev/null 2>"$TARGET_ERROR"
    then
      print -u2 -r -- "wrong-password probe unexpectedly succeeded for $ROLE"
      exit 1
    fi
    grep -F "password authentication failed for user \"$ROLE\"" \
      "$TARGET_ERROR" >/dev/null
    PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
      --no-psqlrc --set ON_ERROR_STOP=1 --password \
      --host 127.0.0.1 --port 5432 --username "$ROLE" \
      --dbname "$STAGING_DATABASE" \
      --command "SELECT * FROM ${READINESS_FUNCTION}()" >/dev/null
    : >"$POSTGRES_ERROR"
    if LC_ALL=C PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
      --no-psqlrc --set ON_ERROR_STOP=1 --password \
      --host 127.0.0.1 --port 5432 --username "$ROLE" \
      --dbname postgres --command 'SELECT 1' \
      >/dev/null 2>"$POSTGRES_ERROR"
    then
      print -u2 -r -- "postgres HBA probe unexpectedly succeeded for $ROLE"
      exit 1
    fi
    grep -F 'pg_hba.conf rejects connection' "$POSTGRES_ERROR" >/dev/null
    grep -F 'database "postgres"' "$POSTGRES_ERROR" >/dev/null
  done
)
```

The correct passwords are read only by `psql` from the terminal. They are
never command arguments, shell-history text, exported variables, or command
output.

Now create one temporary database after bootstrap and deliberately give
`PUBLIC` the future-database privileges against which the HBA boundary must
protect. The fixed name must not already exist:

```zsh
(
  set -euo pipefail
  PROBE_DATABASE=starring_runtime_hba_probe
  : "${STARRING_STAGING_CLUSTER_ADMIN:?load the reviewed cluster administrator}"
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  PROBE_EXISTS="$(
    PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
      --no-psqlrc --set ON_ERROR_STOP=1 --password \
      --host 127.0.0.1 --port 5432 \
      --username "$STARRING_STAGING_CLUSTER_ADMIN" \
      --dbname starring_runtime_staging --tuples-only --no-align \
      --command "SELECT pg_catalog.count(*) FROM pg_catalog.pg_database WHERE datname = '$PROBE_DATABASE'"
  )"
  test "$PROBE_EXISTS" = 0
  PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
    --no-psqlrc --set ON_ERROR_STOP=1 --password \
    --host 127.0.0.1 --port 5432 \
    --username "$STARRING_STAGING_CLUSTER_ADMIN" \
    --dbname postgres \
    --command 'CREATE DATABASE starring_runtime_hba_probe' \
    --command 'GRANT CONNECT, TEMPORARY ON DATABASE starring_runtime_hba_probe TO PUBLIC'
  PUBLIC_PROOF="$(
    PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
      --no-psqlrc --set ON_ERROR_STOP=1 --password \
      --host 127.0.0.1 --port 5432 \
      --username "$STARRING_STAGING_CLUSTER_ADMIN" \
      --dbname starring_runtime_staging --tuples-only --no-align \
      --command "
        SELECT pg_catalog.concat_ws(
          '|',
          pg_catalog.count(*) FILTER (
            WHERE pg_catalog.has_database_privilege(
              rolname,
              '$PROBE_DATABASE',
              'CONNECT'
            )
          ),
          pg_catalog.count(*) FILTER (
            WHERE pg_catalog.has_database_privilege(
              rolname,
              '$PROBE_DATABASE',
              'TEMPORARY'
            )
          )
        )
        FROM pg_catalog.pg_roles
        WHERE rolname IN (
          'starring_runtime_execution',
          'starring_runtime_exact_target',
          'starring_runtime_panel',
          'starring_runtime_serving',
          'starring_runtime_interaction'
        )
      "
  )"
  test "$PUBLIC_PROOF" = '5|5'
)
```

The probe database intentionally makes capability readiness report drift while
it exists. Keep the runtime unloaded. With each correct role password, prove
that the earlier all-database reject rule still denies the database despite
the effective `PUBLIC` privileges:

```zsh
(
  set -euo pipefail
  PROBE_DATABASE=starring_runtime_hba_probe
  ROLES=(
    starring_runtime_execution
    starring_runtime_exact_target
    starring_runtime_panel
    starring_runtime_serving
    starring_runtime_interaction
  )
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  PROBE_ERROR="$(mktemp)"
  trap 'rm -f -- "$PROBE_ERROR"' EXIT
  for ROLE in "${ROLES[@]}"; do
    : >"$PROBE_ERROR"
    if LC_ALL=C PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
      --no-psqlrc --set ON_ERROR_STOP=1 --password \
      --host 127.0.0.1 --port 5432 --username "$ROLE" \
      --dbname "$PROBE_DATABASE" --command 'SELECT 1' \
      >/dev/null 2>"$PROBE_ERROR"
    then
      print -u2 -r -- "future-database HBA probe unexpectedly succeeded for $ROLE"
      exit 1
    fi
    grep -F 'pg_hba.conf rejects connection' "$PROBE_ERROR" >/dev/null
    grep -F "database \"$PROBE_DATABASE\"" "$PROBE_ERROR" >/dev/null
  done
)
```

Drop the temporary database after a successful probe. If any probe fails,
first perform the mandatory quarantine-mode reseal above. Only after all five
roles are `NOLOGIN` with null passwords may the operator record the failing
role and block exit status, investigate, and run this cleanup block:

```zsh
(
  set -euo pipefail
  : "${STARRING_STAGING_CLUSTER_ADMIN:?load the reviewed cluster administrator}"
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
    --no-psqlrc --set ON_ERROR_STOP=1 --password \
    --host 127.0.0.1 --port 5432 \
    --username "$STARRING_STAGING_CLUSTER_ADMIN" \
    --dbname postgres \
    --command 'DROP DATABASE starring_runtime_hba_probe'
)
```

## Store indirect secrets in Keychain

For a loopback PostgreSQL server, each Keychain value is a complete URL in this
accepted form:

```text
postgresql://ROLE:PASSWORD@127.0.0.1:5432/starring_runtime_staging?sslmode=disable
```

Use the matching distinct role and password for each account below. The
runtime rejects URL encoding, ambient `PG*` connection variables, duplicate
role/database identities, missing explicit ports, and unsafe non-loopback
transport. This runbook does not authorize a remote database; that requires a
separate transport and operations review.

Run each command exactly as shown with `-w` last. `/usr/bin/security` prompts
for the full URL without putting it in the command line:

```zsh
(
  set -euo pipefail
  /usr/bin/security add-generic-password -U \
    -s starring.runtime.staging -a database.execution -w
  /usr/bin/security add-generic-password -U \
    -s starring.runtime.staging -a database.exact-target -w
  /usr/bin/security add-generic-password -U \
    -s starring.runtime.staging -a database.panel -w
  /usr/bin/security add-generic-password -U \
    -s starring.runtime.staging -a database.serving -w
  /usr/bin/security add-generic-password -U \
    -s starring.runtime.staging -a database.interaction -w
  /usr/bin/security add-generic-password -U \
    -s starring.runtime.staging -a discord.bot-token -w
)
```

The last prompt receives the Discord bot token, not a URL. Never use `-A`,
never place a value after `-w`, and never export these values.

Keychain keeps values out of the plist, repository, process arguments, and
shell history, but it does not isolate mutually untrusted processes running as
this same macOS login. The default trusted `/usr/bin/security` client can be
invoked by another process under the account. This remains a staging contract;
do not promote it to a shared-login production secret boundary.

Preflight only the existence of each Keychain item. These commands must not
include `-w`, because `find-generic-password -w` prints the secret:

```zsh
(
  set -euo pipefail
  for ACCOUNT in \
    database.execution \
    database.exact-target \
    database.panel \
    database.serving \
    database.interaction \
    discord.bot-token
  do
    /usr/bin/security find-generic-password \
      -s starring.runtime.staging -a "$ACCOUNT" >/dev/null
  done
)
```

## Build and install an immutable revision

The executable rejects a build without its exact 40-character Git revision
compiled in. Build from a clean commit. A successful switch records the
previous immutable target in `~/.local/libexec/starring-runtime.previous`;
failed builds cannot reach the install or symlink steps.

```zsh
(
  set -euo pipefail
  cd /Users/jungbogeon/starring
  test -z "$(git status --porcelain --untracked-files=normal)"
  REVISION="$(git rev-parse HEAD)"
  print -r -- "$REVISION" | grep -Eq '^[0-9a-f]{40}$'
  STARRING_RUNTIME_BUILD_REVISION="$REVISION" \
    cargo build --locked --release -p starring-runtime
  mkdir -p "$HOME/.local/libexec" "$HOME/Library/Logs/starring-runtime"
  chmod 700 "$HOME/.local/libexec" "$HOME/Library/Logs/starring-runtime"
  install -m 500 target/release/starring-runtime \
    "$HOME/.local/libexec/starring-runtime-$REVISION"
  RUNTIME_LINK="$HOME/.local/libexec/starring-runtime"
  PREVIOUS_LINK="$HOME/.local/libexec/starring-runtime.previous"
  if test -L "$RUNTIME_LINK"; then
    PREVIOUS_BINARY="$(readlink "$RUNTIME_LINK")"
    test "${PREVIOUS_BINARY#/}" = "$PREVIOUS_BINARY"
    print -r -- "$PREVIOUS_BINARY" \
      | grep -Eq '^starring-runtime-[0-9a-f]{40}$'
    test -x "$HOME/.local/libexec/$PREVIOUS_BINARY"
    ln -sfn "$PREVIOUS_BINARY" "$PREVIOUS_LINK"
  else
    test ! -e "$RUNTIME_LINK"
  fi
  ln -sfn "starring-runtime-$REVISION" "$RUNTIME_LINK"
  test -x "$RUNTIME_LINK"
)
```

## Install and start the LaunchAgent

```zsh
(
  set -euo pipefail
  cd /Users/jungbogeon/starring
  mkdir -p "$HOME/Library/LaunchAgents"
  INSTALLED_PLIST="$HOME/Library/LaunchAgents/local.starring.runtime.staging.plist"
  if test -f "$INSTALLED_PLIST"; then
    cp -p "$INSTALLED_PLIST" "$INSTALLED_PLIST.previous"
  fi
  install -m 600 ops/macos/local.starring.runtime.staging.plist \
    "$INSTALLED_PLIST"
  plutil -lint "$INSTALLED_PLIST"
  DOMAIN="gui/$(id -u)"
  SERVICE="$DOMAIN/local.starring.runtime.staging"
  if launchctl print "$SERVICE" >/dev/null 2>&1; then
    launchctl bootout "$SERVICE"
  fi
  launchctl enable "$SERVICE"
  launchctl bootstrap "$DOMAIN" "$INSTALLED_PLIST"
  launchctl print "$SERVICE" >/dev/null
)
```

`KeepAlive.SuccessfulExit=false` restarts a failed process but leaves an
intentional clean SIGTERM shutdown stopped. A 30-second throttle prevents a
tight crash loop. `RunAtLoad=true` starts the process during `bootstrap`; do
not follow bootstrap with `kickstart -k`, because `-k` would terminate a newly
started process without its shutdown protocol.

## Health and empty-open verification

First prove the listener is loopback-only and liveness is available:

```zsh
(
  set -euo pipefail
  lsof -nP -iTCP:19091 -sTCP:LISTEN
  curl --fail --silent --show-error --max-time 1 \
    http://127.0.0.1:19091/health/live
)
```

The listener output must name only `127.0.0.1:19091`. Stop immediately if it
shows `*:19091`, `[::]:19091`, or any non-loopback address.

Readiness remains `503 not_ready` while recovery, exact capability checks,
paused Discord connection, owner acquisition, and the durable ingress-open
acknowledgement converge. Allow one bounded startup window:

```zsh
(
  set -euo pipefail
  DOMAIN="gui/$(id -u)"
  SERVICE="$DOMAIN/local.starring.runtime.staging"
  READY=0
  for ATTEMPT in {1..50}; do
    if curl --fail --silent --show-error --max-time 1 \
      http://127.0.0.1:19091/health/ready >/dev/null 2>&1
    then
      READY=1
      break
    fi
    sleep 1
  done
  test "$READY" = 1
  launchctl print "$SERVICE"
  tail -n 100 "$HOME/Library/Logs/starring-runtime/runtime.log"
)
```

The log must contain only finite status codes and contexts, never a database
URL, password, Discord token, or Keychain value. `ready` at this release still
means empty-open only. There is no customer-route smoke request to send.

## SIGTERM acceptance

Exercise the real shutdown path before accepting the installation. A clean
shutdown seals readiness first, drains bounded work, releases the owner, closes
Discord, joins supervisors, closes database pools, and stops health within the
30-second process deadline.

```zsh
(
  set -euo pipefail
  DOMAIN="gui/$(id -u)"
  SERVICE="$DOMAIN/local.starring.runtime.staging"
  LOG_PATH="$HOME/Library/Logs/starring-runtime/runtime.log"
  test -f "$LOG_PATH"
  SERVICE_STATE="$(launchctl print "$SERVICE")"
  PID="$(
    print -r -- "$SERVICE_STATE" \
      | awk '/^[[:space:]]*pid = / { print $3; exit }'
  )"
  RUNS="$(
    print -r -- "$SERVICE_STATE" \
      | awk '/^[[:space:]]*runs = / { print $3; exit }'
  )"
  print -r -- "$PID" | grep -Eq '^[0-9]+$'
  print -r -- "$RUNS" | grep -Eq '^[0-9]+$'
  LOG_INODE="$(stat -f '%i' "$LOG_PATH")"
  LOG_OFFSET="$(stat -f '%z' "$LOG_PATH")"
  SHUTDOWN_STARTED="$(
    /usr/bin/perl \
      -MTime::HiRes=clock_gettime,CLOCK_MONOTONIC \
      -e 'printf "%.9f\n", clock_gettime(CLOCK_MONOTONIC)'
  )"
  launchctl kill SIGTERM "$SERVICE"
  SHUTDOWN_ELAPSED_MILLISECONDS="$(
    /usr/bin/perl \
      -MTime::HiRes=clock_gettime,CLOCK_MONOTONIC,sleep \
      -e '
        my ($pid, $started) = @ARGV;
        while (kill 0, $pid) {
          my $elapsed = clock_gettime(CLOCK_MONOTONIC) - $started;
          exit 124 if $elapsed > 30;
          sleep 0.1;
        }
        my $elapsed = clock_gettime(CLOCK_MONOTONIC) - $started;
        exit 124 if $elapsed > 30;
        printf "%.0f\n", $elapsed * 1000;
      ' \
      "$PID" "$SHUTDOWN_STARTED"
  )"
  print -r -- "$SHUTDOWN_ELAPSED_MILLISECONDS" | grep -Eq '^[0-9]+$'
  test "$SHUTDOWN_ELAPSED_MILLISECONDS" -le 30000
  ! kill -0 "$PID" 2>/dev/null
  test "$(stat -f '%i' "$LOG_PATH")" = "$LOG_INODE"
  LOG_SEGMENT="$(tail -c "+$((LOG_OFFSET + 1))" "$LOG_PATH")"
  STATUS_COUNT="$(
    print -r -- "$LOG_SEGMENT" \
      | grep -c '^starring_runtime_status='
  )"
  test "$STATUS_COUNT" = 1
  print -r -- "$LOG_SEGMENT" \
    | grep -Fx 'starring_runtime_status=runtime_process_clean_shutdown' \
    >/dev/null
  SERVICE_STATE="$(launchctl print "$SERVICE")"
  STOPPED_RUNS="$(
    print -r -- "$SERVICE_STATE" \
      | awk '/^[[:space:]]*runs = / { print $3; exit }'
  )"
  LAST_EXIT_CODE="$(
    print -r -- "$SERVICE_STATE" \
      | awk '/^[[:space:]]*last exit code = / { print $5; exit }'
  )"
  test "$STOPPED_RUNS" = "$RUNS"
  test "$LAST_EXIT_CODE" = 0
  ! print -r -- "$SERVICE_STATE" \
    | grep -Eq '^[[:space:]]*pid = '
  THROTTLE_STARTED="$(
    /usr/bin/perl \
      -MTime::HiRes=clock_gettime,CLOCK_MONOTONIC \
      -e 'printf "%.9f\n", clock_gettime(CLOCK_MONOTONIC)'
  )"
  while /usr/bin/perl \
    -MTime::HiRes=clock_gettime,CLOCK_MONOTONIC \
    -e 'exit(clock_gettime(CLOCK_MONOTONIC) - $ARGV[0] < 31 ? 0 : 1)' \
    "$THROTTLE_STARTED"
  do
    SERVICE_STATE="$(launchctl print "$SERVICE")"
    STOPPED_RUNS="$(
      print -r -- "$SERVICE_STATE" \
        | awk '/^[[:space:]]*runs = / { print $3; exit }'
    )"
    LAST_EXIT_CODE="$(
      print -r -- "$SERVICE_STATE" \
        | awk '/^[[:space:]]*last exit code = / { print $5; exit }'
    )"
    test "$STOPPED_RUNS" = "$RUNS"
    test "$LAST_EXIT_CODE" = 0
    ! print -r -- "$SERVICE_STATE" \
      | grep -Eq '^[[:space:]]*pid = '
    sleep 1
  done
  SERVICE_STATE="$(launchctl print "$SERVICE")"
  STOPPED_RUNS="$(
    print -r -- "$SERVICE_STATE" \
      | awk '/^[[:space:]]*runs = / { print $3; exit }'
  )"
  LAST_EXIT_CODE="$(
    print -r -- "$SERVICE_STATE" \
      | awk '/^[[:space:]]*last exit code = / { print $5; exit }'
  )"
  test "$STOPPED_RUNS" = "$RUNS"
  test "$LAST_EXIT_CODE" = 0
  ! print -r -- "$SERVICE_STATE" \
    | grep -Eq '^[[:space:]]*pid = '
  NEW_PID="$(launchctl kickstart -p "$SERVICE")"
  print -r -- "$NEW_PID" | grep -Eq '^[0-9]+$'
  test "$NEW_PID" != "$PID"
  EXPECTED_RUNS="$(( RUNS + 1 ))"
  READY=0
  for ATTEMPT in {1..50}; do
    SERVICE_STATE="$(launchctl print "$SERVICE")"
    CURRENT_PID="$(
      print -r -- "$SERVICE_STATE" \
        | awk '/^[[:space:]]*pid = / { print $3; exit }'
    )"
    CURRENT_RUNS="$(
      print -r -- "$SERVICE_STATE" \
        | awk '/^[[:space:]]*runs = / { print $3; exit }'
    )"
    test "$CURRENT_PID" = "$NEW_PID"
    test "$CURRENT_RUNS" = "$EXPECTED_RUNS"
    if curl --fail --silent --show-error --max-time 1 \
      http://127.0.0.1:19091/health/ready >/dev/null 2>&1
    then
      READY=1
      break
    fi
    sleep 1
  done
  test "$READY" = 1
  SERVICE_STATE="$(launchctl print "$SERVICE")"
  test "$(
    print -r -- "$SERVICE_STATE" \
      | awk '/^[[:space:]]*pid = / { print $3; exit }'
  )" = "$NEW_PID"
  test "$(
    print -r -- "$SERVICE_STATE" \
      | awk '/^[[:space:]]*runs = / { print $3; exit }'
  )" = "$EXPECTED_RUNS"
)
```

The check reads only bytes appended after signaling the captured PID, requires
exactly one clean-shutdown status in that segment, requires the same launchd
generation to report exit code zero, and enforces the process deadline with a
monotonic timer. The plist's 35-second `ExitTimeOut` is only an outer launchd
kill bound and does not extend the 30-second acceptance limit. The check also
observes the complete 30-second throttle window with no PID or run-count
change. The manual start then requires the PID returned by `kickstart -p` to
remain the same process through readiness and increments the run count exactly
once. Treat a deadline error, an orphaned process, restart, PID succession,
nonzero exit, owner loss, capability-readiness loss, ACK loss, or persistent
`not_ready` as a failed cutover.

## Restore a previously loaded API

Run this block only when the preserved entry evidence says
`api_was_loaded=true`. It restores only the API service that this procedure
stopped and requires both API health gates. If the installed API plist, binary,
or its separate operating authority is unavailable, keep the API stopped and
hand restoration to the owner of the
`2026-07-19-production-control-plane-cutover.md` procedure. Do not silently
finish with a previously loaded API down.

```zsh
(
  set -euo pipefail
  DOMAIN="gui/$(id -u)"
  API_SERVICE="$DOMAIN/local.starring.api.staging"
  API_PLIST="$HOME/Library/LaunchAgents/local.starring.api.staging.plist"
  test -f "$API_PLIST"
  test -x "$HOME/.local/libexec/starring-api"
  plutil -lint "$API_PLIST"
  ! launchctl print "$API_SERVICE" >/dev/null 2>&1
  launchctl bootstrap "$DOMAIN" "$API_PLIST"
  READY=0
  for ATTEMPT in {1..60}; do
    if curl --fail --silent --show-error --max-time 1 \
      http://127.0.0.1:18080/health/live >/dev/null 2>&1 \
      && curl --fail --silent --show-error --max-time 1 \
        http://127.0.0.1:18080/health/ready >/dev/null 2>&1
    then
      READY=1
      break
    fi
    sleep 1
  done
  test "$READY" = 1
  launchctl print "$API_SERVICE"
)
```

If API restoration fails, unload both staging LaunchAgents and immediately run
the earlier quarantine-mode role bootstrap. Keep both services down until the
API owner corrects the failure and the complete password, enable, runtime, and
API readiness sequence is repeated. When `api_was_loaded=false`, do not run the
restore block; record that the API intentionally remained stopped.

## Routine operation

```zsh
(
  set -euo pipefail
  DOMAIN="gui/$(id -u)"
  SERVICE="$DOMAIN/local.starring.runtime.staging"
  launchctl print "$SERVICE"
  curl --fail --silent --show-error --max-time 1 \
    http://127.0.0.1:19091/health/live
  curl --fail --silent --show-error --max-time 1 \
    http://127.0.0.1:19091/health/ready
  tail -n 100 "$HOME/Library/Logs/starring-runtime/runtime.log"
)
```

Do not increase pool sizes, channel capacities, owner lease timings, or drain
timeouts without a new measured SLO cohort. Do not expose port `19091` through
Cloudflare, a router, a public DNS record, or a non-loopback proxy.

## Stop and rollback

An intentional stop uses launchd so the process receives SIGTERM and the
35-second launchd timeout remains outside the runtime's 30-second deadline:

```zsh
(
  set -euo pipefail
  DOMAIN="gui/$(id -u)"
  SERVICE="$DOMAIN/local.starring.runtime.staging"
  SERVICE_STATE="$(launchctl print "$SERVICE")"
  PID="$(
    print -r -- "$SERVICE_STATE" \
      | awk '/^[[:space:]]*pid = / { print $3; exit }'
  )"
  launchctl bootout "$SERVICE"
  if test -n "$PID"; then
    for ATTEMPT in {1..35}; do
      if ! kill -0 "$PID" 2>/dev/null; then
        break
      fi
      sleep 1
    done
    ! kill -0 "$PID" 2>/dev/null
  fi
  ! launchctl print "$SERVICE" >/dev/null 2>&1
)
```

To roll back the executable, keep the service stopped, restore the previous
immutable target and optional previous plist, validate the plist, then start
it:

```zsh
(
  set -euo pipefail
  DOMAIN="gui/$(id -u)"
  SERVICE="$DOMAIN/local.starring.runtime.staging"
  ! launchctl print "$SERVICE" >/dev/null 2>&1
  RUNTIME_LINK="$HOME/.local/libexec/starring-runtime"
  PREVIOUS_LINK="$HOME/.local/libexec/starring-runtime.previous"
  test -L "$PREVIOUS_LINK"
  PREVIOUS_BINARY="$(readlink "$PREVIOUS_LINK")"
  test "${PREVIOUS_BINARY#/}" = "$PREVIOUS_BINARY"
  print -r -- "$PREVIOUS_BINARY" \
    | grep -Eq '^starring-runtime-[0-9a-f]{40}$'
  test -x "$HOME/.local/libexec/$PREVIOUS_BINARY"
  ln -sfn "$PREVIOUS_BINARY" "$RUNTIME_LINK"
  INSTALLED_PLIST="$HOME/Library/LaunchAgents/local.starring.runtime.staging.plist"
  if test -f "$INSTALLED_PLIST.previous"; then
    install -m 600 "$INSTALLED_PLIST.previous" "$INSTALLED_PLIST"
  fi
  plutil -lint "$INSTALLED_PLIST"
  launchctl bootstrap "$DOMAIN" "$INSTALLED_PLIST"
  launchctl print "$SERVICE" >/dev/null
)
```

Then repeat the listener, liveness, readiness, and SIGTERM checks. Database
migrations are forward-only and this role bootstrap has no automatic database
rollback. If the previous executable is not compatible with the current
schema, leave the empty-open service stopped and restore a separately verified
database snapshot instead of improvising reverse SQL.

To revoke the staging runtime completely, stop the LaunchAgent, disable its
label, delete all six `starring.runtime.staging` Keychain items by exact
service/account, and ask a database administrator to return the five login
roles to `NOLOGIN`. Never delete Keychain items while a rollback decision is
still pending.
