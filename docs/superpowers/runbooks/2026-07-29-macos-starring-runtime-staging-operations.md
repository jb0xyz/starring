# macOS Starring Runtime Staging Operations

This runbook installs and operates `starring-runtime` as the logged-in Mac
mini user's LaunchAgent. The staging service is
`local.starring.runtime.staging`, its health listener is loopback-only at
`127.0.0.1:19091`, and every database URL, the Discord bot token, and the
interaction-token envelope keyring are resolved indirectly from macOS
Keychain.

This runtime can acquire and renew the production owner, connect the canonical
Discord shard, reconstruct durable routes, converge Requested deployments,
serve admitted interactions, and recover bounded receipt and effect work. The
B6 staging milestone proved one exact route through Live and real interactions.
That proof is not the Phase D commercial certificate. Process `ready` means the
runtime's process-wide authorities and supervisors are current; it never proves
that a particular installation or route is Live. Use the product deployment
status for the exact installation and promotion before describing customer
traffic as serving.

The current source contract is 125 migrations through
`202608040004_refresh_serving_pending_product_drain_readiness_v1.sql`, 198
owned user-schema relations, and 137 capability functions. Historical D1
evidence at 117 migrations and 135 functions remains valid only for its dated
candidate.

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
| runtime Keychain inventory | exactly 7 service/account items |
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
continue only when its exit status is zero. Secret input follows the contract
of each block: legacy bootstrap prompts use hidden terminal input, while
reviewed incremental paths read only their fixed Keychain items and never
prompt on the server.

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

## Backfill C1 interaction receipt function ACLs

Use this incremental path only when an already-provisioned staging cluster has
applied migration `202607310022` but the five runtime role bootstrap must not
be rerun because its existing SCRAM credentials must remain unchanged. A new
cluster should use the full bootstrap path below instead. Keep both staging
LaunchAgents unloaded. The script rejects any other client backend or prepared
transaction, validates the exact 115-entry repository migration ledger, and
holds a transaction advisory lock for the whole operation.

The transaction can change only the ACLs of the 17 C1 receipt-boundary
functions. It removes `PUBLIC` and unrelated grants, gives the 11 exported
receipt capabilities non-grantable `EXECUTE` to
`starring_runtime_interaction`, and leaves the six internal manifest, claim
helper, and trigger guard functions owner-only. It snapshots every PostgreSQL
role attribute including the SCRAM verifier for `starring_owner` and
`starring_runtime_interaction`, proves the snapshot is unchanged before
commit, and runs interaction database readiness under the actual interaction
session identity. A failed check rolls back every ACL change. An exact replay
is safe and produces the same ACL topology.

```zsh
(
  set -euo pipefail
  set +x
  umask 077
  cd /Users/jungbogeon/starring
  STAGING_DATABASE=starring_runtime_staging
  DOMAIN="gui/$(id -u)"
  ADMIN_PGPASS_DIR=
  ADMIN_PGPASS_PATH=
  cleanup_admin_pgpass() {
    CLEANUP_STATUS=0
    if test -n "${ADMIN_PGPASS_PATH:-}" \
      && test -e "$ADMIN_PGPASS_PATH"
    then
      if test -f "$ADMIN_PGPASS_PATH" \
        && ! test -L "$ADMIN_PGPASS_PATH"
      then
        /bin/dd if=/dev/zero of="$ADMIN_PGPASS_PATH" \
          bs=4096 count=1 conv=notrunc >/dev/null 2>&1 \
          || CLEANUP_STATUS=1
      else
        CLEANUP_STATUS=1
      fi
      /bin/rm -f "$ADMIN_PGPASS_PATH" >/dev/null 2>&1 \
        || CLEANUP_STATUS=1
    fi
    if test -n "${ADMIN_PGPASS_DIR:-}"
    then
      /bin/rmdir "$ADMIN_PGPASS_DIR" >/dev/null 2>&1 \
        || CLEANUP_STATUS=1
    fi
    return "$CLEANUP_STATUS"
  }
  trap \
    'TRAP_STATUS=$?; trap - EXIT HUP INT TERM; cleanup_admin_pgpass || CLEANUP_STATUS=$?; if test "$TRAP_STATUS" -eq 0 && test "${CLEANUP_STATUS:-0}" -ne 0; then exit "$CLEANUP_STATUS"; fi; exit "$TRAP_STATUS"' \
    EXIT
  trap 'exit 129' HUP
  trap 'exit 130' INT
  trap 'exit 143' TERM
  : "${STARRING_STAGING_CLUSTER_ADMIN:?load the reviewed cluster administrator}"
  : "${STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER:?load the reviewed system identifier}"
  : "${STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT:?load the reviewed dedicated-cluster acknowledgement}"
  test "$STARRING_STAGING_CLUSTER_ADMIN" = starring_cluster_admin
  ! launchctl print "$DOMAIN/local.starring.runtime.staging" >/dev/null 2>&1
  ! launchctl print "$DOMAIN/local.starring.api.staging" >/dev/null 2>&1
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  ADMIN_PGPASS_DIR="$(
    /usr/bin/mktemp -d /private/tmp/starring-admin-pgpass.XXXXXX
  )"
  test -d "$ADMIN_PGPASS_DIR"
  test "$(
    /usr/bin/stat -f '%u:%Lp' "$ADMIN_PGPASS_DIR"
  )" = "$(/usr/bin/id -u):700"
  /bin/ls -lde "$ADMIN_PGPASS_DIR" \
    | /usr/bin/awk \
      'NR == 1 { ok = ($1 == "drwx------" || $1 == "drwx------@") } NR > 1 { ok = 0 } END { exit(ok ? 0 : 1) }'
  ADMIN_PGPASS_PATH="$ADMIN_PGPASS_DIR/pgpass"
  /usr/bin/security find-generic-password -w \
    -s starring.postgres.staging \
    -a database.cluster-admin \
    | /usr/bin/sed -nE \
      's#^postgresql://starring_cluster_admin:([A-Za-z0-9_-]{43})@127\.0\.0\.1:5432/postgres\?sslmode=disable$#127.0.0.1:5432:*:starring_cluster_admin:\1#p' \
      >"$ADMIN_PGPASS_PATH"
  test -f "$ADMIN_PGPASS_PATH"
  ! test -L "$ADMIN_PGPASS_PATH"
  test "$(
    /usr/bin/stat -f '%u:%Lp' "$ADMIN_PGPASS_PATH"
  )" = "$(/usr/bin/id -u):600"
  /bin/ls -le "$ADMIN_PGPASS_PATH" \
    | /usr/bin/awk \
      'NR == 1 { ok = ($1 == "-rw-------" || $1 == "-rw-------@") } NR > 1 { ok = 0 } END { exit(ok ? 0 : 1) }'
  test "$(
    /usr/bin/wc -l <"$ADMIN_PGPASS_PATH" | /usr/bin/tr -d ' '
  )" = 1
  /usr/bin/grep -Eq \
    '^127\.0\.0\.1:5432:\*:starring_cluster_admin:[A-Za-z0-9_-]{43}$' \
    "$ADMIN_PGPASS_PATH"
  PGPASSFILE="$ADMIN_PGPASS_PATH" PGSSLMODE=disable \
    /opt/homebrew/opt/postgresql@16/bin/psql \
    --no-psqlrc --set ON_ERROR_STOP=1 --no-password \
    --host 127.0.0.1 --port 5432 \
    --username "$STARRING_STAGING_CLUSTER_ADMIN" \
    --dbname "$STAGING_DATABASE" \
    --set expected_database="$STAGING_DATABASE" \
    --set expected_system_identifier="$STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER" \
    --set runtime_dedicated_cluster_acknowledgement="$STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT" \
    --file ops/postgres/staging-runtime-interaction-receipt-acl-backfill.sql
)
```

Do not substitute a database URL, password environment variable, inline SQL
credential, or interactive server prompt. The administrator URL flows from the
fixed Keychain item through a mode-`0600` temporary `PGPASSFILE`; the trap
overwrites and removes that file on success, error, or signal. Do not use this
script if any migration later than `202607310022` exists; update and
independently review its fixed ledger and function manifests first.

For an existing credentialed cluster, a successful C1 backfill completes the
runtime-role ACL step. Skip the following bootstrap, quarantine, password
creation, role-enable, and credential-rotation sections; resuming any of those
fresh-cluster sections would intentionally reset the existing runtime role
credentials. Continue at `Store indirect secrets in Keychain` and use only its
dedicated legacy interaction-token keyring mode before building the immutable
revision.

## Backfill C3 interaction effect function ACLs

Use this additive path at migration head `202608040004` when an existing
credentialed cluster must receive the exact effect-journal capability ACLs
without rotating its runtime credentials or rerunning either role bootstrap.
Keep API and runtime unloaded. The script requires cluster-wide zero client
backends and zero prepared transactions, verifies the exact 125-entry ledger
and migration checksum, preserves both role attribute and SCRAM-verifier
snapshots, and rechecks quiescence before commit.

The fixed manifest contains 22 functions. Eleven external capabilities receive
non-grantable `EXECUTE` for `starring_runtime_interaction`; eleven guards,
manifest helpers, and cross-record functions remain owner-only. `PUBLIC`,
unrelated roles, grant options, direct relations, role membership, role
settings, and owner drift fail closed. Exact replay produces the same ACL.

Use the complete Keychain-to-mode-`0600` temporary `PGPASSFILE` setup and trap
from the C1 section, but execute only this current-head script at the final
`psql` step:

```zsh
PGPASSFILE="$ADMIN_PGPASS_PATH" PGSSLMODE=disable \
  /opt/homebrew/opt/postgresql@16/bin/psql \
  --no-psqlrc --set ON_ERROR_STOP=1 --no-password \
  --host 127.0.0.1 --port 5432 \
  --username "$STARRING_STAGING_CLUSTER_ADMIN" \
  --dbname "$STAGING_DATABASE" \
  --set expected_database="$STAGING_DATABASE" \
  --set expected_system_identifier="$STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER" \
  --set runtime_dedicated_cluster_acknowledgement="$STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT" \
  --file ops/postgres/staging-runtime-interaction-effect-acl-backfill.sql
```

Do not run the older C1 SQL file in the same transaction or leave either
LaunchAgent loaded. On failure, retain only the stable error, prove the
transaction rolled back, and keep both services stopped.

## Inspect duplicate receipts without exposing identities

The loopback-only interaction health projection is a process-local, redacted
counter view. It contains no application, guild, installation, interaction,
route, user, token, payload, or effect identity. Capture it before and after a
reviewed duplicate-delivery drill to classify the receipt result without
querying receipt tables or copying Discord interaction data into evidence:

```zsh
(
  set -euo pipefail
  SNAPSHOT="$(
    curl --fail --silent --show-error --max-time 1 \
      http://127.0.0.1:19091/health/interactions
  )"
  print -r -- "$SNAPSHOT" | jq -e '
    [
      .receipt_acquired,
      .receipt_completed_duplicate,
      .receipt_in_flight_duplicate,
      .receipt_terminal_duplicate,
      .receipt_recovery_required_duplicate,
      .receipt_claim_closed,
      .receipt_claim_timeout,
      .receipt_claim_unavailable,
      .receipt_claim_rejected,
      .receipt_claim_corrupt,
      .receipt_authority_rejected,
      .receipt_persistence_failed_before_effect,
      .receipt_persistence_failed_after_effect,
      .receipt_terminal_recovery_required,
      .in_flight
    ] | all(type == "number" and . >= 0)
  ' >/dev/null
  print -r -- "$SNAPSHOT" | jq '{
    receipt_acquired,
    receipt_completed_duplicate,
    receipt_in_flight_duplicate,
    receipt_terminal_duplicate,
    receipt_recovery_required_duplicate,
    receipt_claim_closed,
    receipt_claim_timeout,
    receipt_claim_unavailable,
    receipt_claim_rejected,
    receipt_claim_corrupt,
    receipt_authority_rejected,
    receipt_persistence_failed_before_effect,
    receipt_persistence_failed_after_effect,
    receipt_terminal_recovery_required,
    in_flight
  }'
)
```

The counters are monotonic only for the current runtime process and reset after
a restart. A duplicate counter increase proves the runtime classified a replay;
it does not alone prove that Discord observed one mutable effect. Pair it with
the disposable-guild resource count and the durable final receipt/effect state
required by the Phase D E2E. Never use direct table reads, raw interaction IDs,
or token ciphertext as routine operational evidence.

## Inspect durable interaction effect recovery blocks

Run this read-only inspection at migration head `202608040004` when
an interaction route remains blocked or before deciding whether manual
recovery is appropriate. The inspection uses one repeatable-read transaction,
validates PostgreSQL 16, the fixed staging database and dedicated cluster, the
exact 125-entry migration ledger, and the interaction-effect schema manifest.
It then emits only recovery block code, action kind, aggregate count, and the
oldest and newest block times. It never emits application, interaction,
action, or Discord output identifiers, nor digest, correlation,
response-token, input, preimage, or payload values. An unknown code or a
recovery-required head without its exact terminal event makes the command fail
before any projection is printed.

The runtime and API may remain loaded because this operation neither locks the
service boundary nor changes database state. A zero-row result means there are
no current recovery-required effects in the transaction snapshot.

```zsh
(
  set -euo pipefail
  set +x
  umask 077
  cd /Users/jungbogeon/starring
  STAGING_DATABASE=starring_runtime_staging
  ADMIN_PGPASS_DIR=
  ADMIN_PGPASS_PATH=
  cleanup_admin_pgpass() {
    CLEANUP_STATUS=0
    if test -n "${ADMIN_PGPASS_PATH:-}" \
      && test -e "$ADMIN_PGPASS_PATH"
    then
      if test -f "$ADMIN_PGPASS_PATH" \
        && ! test -L "$ADMIN_PGPASS_PATH"
      then
        /bin/dd if=/dev/zero of="$ADMIN_PGPASS_PATH" \
          bs=4096 count=1 conv=notrunc >/dev/null 2>&1 \
          || CLEANUP_STATUS=1
      else
        CLEANUP_STATUS=1
      fi
      /bin/rm -f "$ADMIN_PGPASS_PATH" >/dev/null 2>&1 \
        || CLEANUP_STATUS=1
    fi
    if test -n "${ADMIN_PGPASS_DIR:-}"
    then
      /bin/rmdir "$ADMIN_PGPASS_DIR" >/dev/null 2>&1 \
        || CLEANUP_STATUS=1
    fi
    return "$CLEANUP_STATUS"
  }
  trap \
    'TRAP_STATUS=$?; trap - EXIT HUP INT TERM; cleanup_admin_pgpass || CLEANUP_STATUS=$?; if test "$TRAP_STATUS" -eq 0 && test "${CLEANUP_STATUS:-0}" -ne 0; then exit "$CLEANUP_STATUS"; fi; exit "$TRAP_STATUS"' \
    EXIT
  trap 'exit 129' HUP
  trap 'exit 130' INT
  trap 'exit 143' TERM
  : "${STARRING_STAGING_CLUSTER_ADMIN:?load the reviewed cluster administrator}"
  : "${STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER:?load the reviewed system identifier}"
  : "${STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT:?load the reviewed dedicated-cluster acknowledgement}"
  test "$STARRING_STAGING_CLUSTER_ADMIN" = starring_cluster_admin
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  ADMIN_PGPASS_DIR="$(
    /usr/bin/mktemp -d /private/tmp/starring-admin-pgpass.XXXXXX
  )"
  test -d "$ADMIN_PGPASS_DIR"
  test "$(
    /usr/bin/stat -f '%u:%Lp' "$ADMIN_PGPASS_DIR"
  )" = "$(/usr/bin/id -u):700"
  /bin/ls -lde "$ADMIN_PGPASS_DIR" \
    | /usr/bin/awk \
      'NR == 1 { ok = ($1 == "drwx------" || $1 == "drwx------@") } NR > 1 { ok = 0 } END { exit(ok ? 0 : 1) }'
  ADMIN_PGPASS_PATH="$ADMIN_PGPASS_DIR/pgpass"
  /usr/bin/security find-generic-password -w \
    -s starring.postgres.staging \
    -a database.cluster-admin \
    | /usr/bin/sed -nE \
      's#^postgresql://starring_cluster_admin:([A-Za-z0-9_-]{43})@127\.0\.0\.1:5432/postgres\?sslmode=disable$#127.0.0.1:5432:*:starring_cluster_admin:\1#p' \
      >"$ADMIN_PGPASS_PATH"
  test -f "$ADMIN_PGPASS_PATH"
  ! test -L "$ADMIN_PGPASS_PATH"
  test "$(
    /usr/bin/stat -f '%u:%Lp' "$ADMIN_PGPASS_PATH"
  )" = "$(/usr/bin/id -u):600"
  /bin/ls -le "$ADMIN_PGPASS_PATH" \
    | /usr/bin/awk \
      'NR == 1 { ok = ($1 == "-rw-------" || $1 == "-rw-------@") } NR > 1 { ok = 0 } END { exit(ok ? 0 : 1) }'
  test "$(
    /usr/bin/wc -l <"$ADMIN_PGPASS_PATH" | /usr/bin/tr -d ' '
  )" = 1
  /usr/bin/grep -Eq \
    '^127\.0\.0\.1:5432:\*:starring_cluster_admin:[A-Za-z0-9_-]{43}$' \
    "$ADMIN_PGPASS_PATH"
  PGPASSFILE="$ADMIN_PGPASS_PATH" PGSSLMODE=disable \
    /opt/homebrew/opt/postgresql@16/bin/psql \
    --no-psqlrc --set ON_ERROR_STOP=1 --no-password \
    --host 127.0.0.1 --port 5432 \
    --username "$STARRING_STAGING_CLUSTER_ADMIN" \
    --dbname "$STAGING_DATABASE" \
    --set expected_database="$STAGING_DATABASE" \
    --set expected_system_identifier="$STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER" \
    --set runtime_dedicated_cluster_acknowledgement="$STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT" \
    --file ops/postgres/staging-runtime-interaction-effect-inspection.sql
)
```

Do not replace the fixed Keychain-to-`PGPASSFILE` path with a database URL,
password environment variable, command-line password, inline credential, or
interactive prompt. Do not replay an interaction, delete an effect row, or
edit a journal row in response to this projection. Preserve the durable
journal and use the action below for the exact reported code.

| Recovery block code | Required operator action |
| --- | --- |
| `recovery_blocked_discord_read_rejected` | Keep the route blocked, inspect Discord connectivity and audit-read permission, repair the external read path, then rerun this inspection before considering recovery. |
| `recovery_blocked_response_token_unavailable` | Preserve the receipt and effect history, do not fabricate a response, and treat response delivery as unrecoverable while verifying the product-visible state. |
| `recovery_blocked_observation_protocol` | Stop automatic recovery for the route, preserve the journal, compare the observed Discord state with the expected protocol shape, and escalate as a code defect. |
| `recovery_blocked_compensation_conflict` | Do not delete external state; compare the target with the preserved preimage and perform only a documented manual reconciliation. |
| `recovery_blocked_compensation_unsupported` | Leave the route blocked and choose a documented manual remediation; neither retry nor delete the external resource. |
| `recovery_blocked_non_compensable` | Preserve the external state and journal, avoid automatic deletion, and resolve the product state through a documented manual procedure. |
| `recovery_blocked_internal_conflict` | Keep the route blocked, verify installation, instance, and route authority through supported product reads, and never override durable records. |
| `recovery_blocked_discord_forbidden` | Restore the intended bot permission through Discord administration and do not retry the mutation until exact authority has been re-established. |
| `recovery_blocked_internal_authority` | Keep the route blocked and restore or rotate authority through the reviewed product or operator path; never repair it with direct table edits. |
| `recovery_blocked_attempt_budget_exhausted` | Leave automatic retries stopped, inspect exact current external state, and choose a documented manual remediation before any new attempt. |

`recovery_required` is a durable safety state, not a transient HTTP retry hint.
The exact unsafe route remains blocked while unrelated routes may continue. Do
not clear it by deleting a receipt, effect head, event, or resource. The
15-second recovery supervisor may observe or compensate only within its
database-enforced bounded protocol. Three consecutive failed sweeps make the
supervisor unhealthy and the serving-process revalidation fails closed; a
supervisor exit is also not ready. Preserve the redacted inspection result,
repair only the classified dependency or authority, and let the reviewed
recovery path advance durable state. Escalate any code without a documented
operator action as a release-blocking protocol defect.

## D2 sealed checkpoints and resource inventory

The following boundary is for the isolated D2 database only. Never pass the
standing staging manifest to the D2 sealed provisioner. For each applicable
step, run the exact immutable candidate with one of the closed checkpoints and
write stdout directly to an owner-controlled mode-`0600` evidence file:

```zsh
CHECKPOINT=authoring
"$D2_SEALED_PROVISIONER" inspect \
  --manifest "$D2_MANIFEST" \
  --checkpoint "$CHECKPOINT" \
  >"$D2_EVIDENCE/db-${CHECKPOINT}.json"
chmod 0600 "$D2_EVIDENCE/db-${CHECKPOINT}.json"
```

At the corresponding step, replace `authoring` only with `live`, `interaction`,
`duplicate`, `restart`, `reconciliation`, `replacement`, `precleanup`, or
`absence`. Do not run future
checkpoints early merely to fill files. The inspector uses a
read-only repeatable-read transaction and validates the run, installation,
database, role, migration, route, serving, receipt, effect, and replacement
identity required by that checkpoint. Its envelopes are redacted projections;
they contain no database URL, token, cookie, CSRF value, prompt, transcript,
RuleSet, effect input, preimage, or response credential.

The certification transport's private control protocol exposes one
`starring.d2.run-owned-resource-inventory.v1` projection. It is bound to the
transport process instance, run, guild, hub, actor, and bot; contains at most
128 sorted role, channel, and message history entries; allows only one
Created-to-Deleted transition per identity; derives Created, Deleted, and
Active sets; and seals the canonical projection with SHA-256. The D2 platform
adapter, not an ad hoc socket client, validates and collects it. Cleanup may
observe or delete only an identity in its Created set and must retain the
post-delete observation. A qualifying runtime deletion must also traverse the
pinned transport so the lifecycle becomes Deleted; out-of-band absence does not
rewrite the inventory. The final inventory must have zero Active entries. The
guild, hub, application, actor, and bot identities are protected and can never
be treated as run-owned child resources.

An inventory is necessary but not sufficient evidence: join it to the sealed
database checkpoint, public product envelope, fault snapshot, and visible
Discord observation required by the numbered D2 receipt. A transport restart
changes the instance identity and invalidates the run instead of creating a new
empty inventory.

For step 9, the sealed checkpoint must show completed create and join receipts,
the manifest actor, the created instance, one exact successful membership of
the created role in each path, and one successful ephemeral acknowledgement per
receipt. The Chrome observation independently names the same actor,
interactions, joined role, one role, one channel, affirmatively observed welcome
panel, and affirmatively observed hub join panel, and the transport inventory
must match all four resource identities.

Across timed checkpoints, compare the receipt-level route lineage rather than
the complete raw route snapshot. Serving heartbeats and gateway-owner renewals
legitimately increase their two revision counters; Step 9, the Step 12 source,
and Step 13 must carry non-regressing values while retaining the same stable
deployment, generation, fence, incarnation, process, lease epochs, and shard.
Step 14 uses its attestation's initial revision projection, so it is joined by
stable lineage without a counter-order comparison. The sealed source digest
still binds every raw field, and any stable-field drift remains a hard stop.

For step 15, one durable partition operation and one durable heal operation
must belong to the same pinned transport instance. The partition counter is
exactly one; the public projection is HTTP `200/200`, product `pending`,
operational `pending`, runtime phase `live`, serving `disconnected`, closed code
`runtime_gateway_disconnected`, and `retryable=true` while partitioned; the heal
completion restores readiness 200 with partition false and all duplicate and
indeterminate arm and claim flags false.

Certified cleanup begins only after the coordinator's step-15 completion is
durable. It writes a freeze intent, stops ingress and runtime, checks that the
resource inventory did not change across that boundary, deletes the frozen
resources, and records step 16 before destroying the remaining run substrate.
The generic cleanup path validates the root, journal, lock, owner, mode, and
non-symlink identities before its first mutation. Running standalone Discord
teardown writes an abort tombstone and permanently disqualifies the run. Step
17 must occur after the exact step-16 completion and join database absence,
Chrome prefix absence, Chrome guild deletion, and orchestrator absence.

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
    -s starring.runtime.staging -a interaction.token-envelope-keyring -w
  /usr/bin/security add-generic-password -U \
    -s starring.runtime.staging -a discord.bot-token -w
)
```

The interaction-token prompt accepts only this payload shape:

```text
v1;active=KEY_ID=64_LOWERCASE_HEX;retired=KEY_ID=64_LOWERCASE_HEX,...
```

The active key plus retired keys must total at most eight. IDs must be bounded
and unique, and every 64-character lowercase hexadecimal material value must
decode to a distinct non-repetitive 32-byte key. Fresh integrated staging uses
the one-shot provisioner to generate and write this item in the same rollback
boundary as the other managed items. The interactive command is only for a
separately reviewed standalone provision or rotation payload prepared without
shell variables, arguments, history, clipboard evidence, or terminal logging.
Record only its key ID.

For an already-live legacy staging cluster missing only this item, keep the API
and runtime stopped and use the staging provisioner's
`--provision-interaction-token-keyring` mode. It rejects every ambient `PG*`
variable, performs no database connection or mutation, revalidates both API
keyrings, creates only the absent runtime item, and returns exact replay without
rotation when the existing item is valid. Do not use the interactive command
or rerun the one-shot provisioner for that backfill.

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
    interaction.token-envelope-keyring \
    discord.bot-token
  do
    /usr/bin/security find-generic-password \
      -s starring.runtime.staging -a "$ACCOUNT" >/dev/null
  done
)
```

## Rotate runtime secrets

Every runtime secret-rotation inventory contains exactly seven accounts:

```text
database.execution
database.exact-target
database.panel
database.serving
database.interaction
interaction.token-envelope-keyring
discord.bot-token
```

Keep the runtime and API stopped for database or shared Discord-token
rotation. The interaction-token envelope keyring has its own staged rotation
contract. Create a new independent active key, move the previous active key to
the retired list, retain every still-required retired key, and keep the total
at eight or fewer. Apply the complete payload as one Keychain item through a
separately reviewed rollback-capable operation. Do not rerun the one-shot fresh
provisioner and do not update active and retired material in separate writes.

After the update, run the final provisioner verifier while applications remain
stopped, then start the runtime and repeat liveness, readiness, and SIGTERM
acceptance. The verifier and runtime may report only key IDs or stable error
codes. Keep the former active key retired for at least the 15-minute maximum
Discord interaction-token lifetime and until receipt cleanup has independently
proved that no recoverable token uses it. Removing a retired key is a second
reviewed rotation with the same stop, semantic verification, startup, and
rollback gates. Never exceed seven retired keys and never copy key material or
its hash into a change record.

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

## Health and serving-process verification

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
URL, password, Discord token, or Keychain value. Process readiness is not a
route projection. For a route-bearing acceptance, independently require the
exact product deployment status to report Live with fresh serving evidence and
exercise only the reviewed disposable-guild interaction path.

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

## Backup, restore, and failure-drill contract

Take a database backup before every migration or capability-ACL change. Close
public ingress first, then stop API and runtime, apply the cluster-quiescence
proof above, and require zero other client backends and zero prepared
transactions. Create a PostgreSQL 16 custom-format dump as the reviewed cluster
administrator through the same Keychain-to-mode-`0600` `PGPASSFILE` boundary
used by the effect inspection. The backup file and its directory must be mode
`0600` and `0700`, respectively. Record only:

- UTC backup identifier
- source Git revision and binary SHA-256
- migration head and successful ledger count
- dump byte count and SHA-256
- `pg_restore --list` success

Do not record the administrator URL, `PGPASSFILE`, Keychain output, relation
rows, receipt or effect identities, encrypted payloads, or Discord identifiers.
A dump that has not passed a restore drill is not a verified backup.

Restore only into an isolated PostgreSQL 16 cluster or a unique disposable
database with all staging clients and public ingress unable to reach it. Never
restore over `starring_runtime_staging`. Require `pg_restore --exit-on-error` to
finish, diff the restored `_sqlx_migrations` versions and checksums against the
repository, verify the common owner and function manifests, and run the exact
capability readiness suites before deleting the disposable restore target. A
different database name may intentionally fail application database-identity
readiness; that is not permission to weaken the identity contract. A production
recovery uses a reviewed replacement cluster with the original logical database
identity, then reruns HBA, role, Keychain-reference, API, runtime, route, and
serving checks before ingress reopens.

For a full-cluster cutover backup and restoration, use Gate 2 and the rollback
sequence in
[macOS Starring Integrated Staging Cutover](./2026-07-29-macos-starring-integrated-staging-cutover.md).
Those steps archive the prior data directory without deleting it. Do not mix a
logical dump rollback with an unreviewed reverse migration.

Run failure drills only on the reviewed staging or disposable-guild target. For
each drill, capture the pre-state, inject exactly one failure, observe the
closed state, remove the injection, and prove bounded convergence before moving
to the next case. The release cohort must cover database loss before claim and
before effect, Discord loss before effect, indeterminate Discord outcome,
gateway disconnect, owner and controller lease loss, writer-fence and authority
changes, binding-map drift, process kill/restart, and duplicate HTTP and Discord
delivery. Stop a drill immediately on false Live, a stale writer, a second
mutable effect, missing durable recovery state, or an automatic deletion not
authorized by an exact preimage. Keep the exact checkpoint and stable redacted
code; never retain injected secrets or full interaction payloads.

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
schema, leave the serving process stopped and restore a separately verified
database snapshot instead of improvising reverse SQL. Binary rollback never
rewinds a durable route, receipt, effect journal, or deployment by itself.

To revoke the staging runtime completely, stop the LaunchAgent, disable its
label, delete all seven `starring.runtime.staging` Keychain items by exact
service/account, and ask a database administrator to return the five login
roles to `NOLOGIN`. Never delete Keychain items while a rollback decision is
still pending. The exact removal inventory is:

```zsh
(
  set -euo pipefail
  DOMAIN="gui/$(id -u)"
  SERVICE="$DOMAIN/local.starring.runtime.staging"
  ! launchctl print "$SERVICE" >/dev/null 2>&1
  launchctl disable "$SERVICE"
  for ACCOUNT in \
    database.execution \
    database.exact-target \
    database.panel \
    database.serving \
    database.interaction \
    interaction.token-envelope-keyring \
    discord.bot-token
  do
    /usr/bin/security delete-generic-password \
      -s starring.runtime.staging -a "$ACCOUNT" >/dev/null
  done
)
```
