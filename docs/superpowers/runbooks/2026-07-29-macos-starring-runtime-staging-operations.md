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
starring-runtime-dedicated-staging-cluster-v1:SYSTEM_IDENTIFIER:starring_runtime_staging:cluster-wide-public-acl-reset
```

Every `zsh` block below is a fail-fast subshell. Run a block as one unit and
continue only when its exit status is zero. Secrets are entered only at an
interactive password prompt.

## Preconditions and target validation

```zsh
(
  set -euo pipefail
  cd /Users/jungbogeon/starring
  STAGING_DATABASE=starring_runtime_staging
  : "${STARRING_STAGING_CLUSTER_ADMIN:?load the reviewed cluster administrator}"
  : "${STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER:?load the reviewed system identifier}"
  : "${STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT:?load the reviewed dedicated-cluster acknowledgement}"
  EXPECTED_ACKNOWLEDGEMENT="starring-runtime-dedicated-staging-cluster-v1:${STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER}:${STAGING_DATABASE}:cluster-wide-public-acl-reset"
  test "$STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT" = "$EXPECTED_ACKNOWLEDGEMENT"
  print -r -- "$STARRING_STAGING_CLUSTER_ADMIN" \
    | grep -Eq '^[a-z_][a-z0-9_]{0,62}$'
  print -r -- "$STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER" \
    | grep -Eq '^[0-9]+$'
  test -z "$(git status --porcelain --untracked-files=normal)"
  test "$(git rev-parse --is-inside-work-tree)" = true
  test -x /opt/homebrew/opt/postgresql@16/bin/psql
  test -x /usr/bin/security
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

## Stop the existing service before database work

Bootstrap, migration, and credential rotation require the runtime to be
unloaded. The SQL guard independently refuses any active session or prepared
transaction owned by one of the five roles.

```zsh
(
  set -euo pipefail
  DOMAIN="gui/$(id -u)"
  SERVICE="$DOMAIN/local.starring.runtime.staging"
  if SERVICE_STATE="$(launchctl print "$SERVICE" 2>/dev/null)"; then
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
)
```

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

Run the script in quarantine mode. It verifies the independently supplied
database, system identifier, and dedicated-cluster acknowledgement before any
cluster-wide change. It also proves that the LaunchAgent is unloaded and SQL
then proves that the target roles have no active session or prepared
transaction.

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
    --file ops/postgres/staging-runtime-role-bootstrap.sql
)
```

Bootstrap mode is deliberately fail-closed: it leaves the five roles
`NOLOGIN` with no password. Generate a separate password for every role in a
password manager. Each password must contain 24–512 characters from only
`A-Z`, `a-z`, `0-9`, `_`, `-`, `.`, and `~`; 32 or more random characters is
the operational minimum. Use the client-side `\password` command so plaintext
is not placed in SQL, server statement logs, process arguments, or shell
history:

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

Run the same script in enable mode. It requires five SCRAM-SHA-256 verifiers,
revalidates the exact ACL boundary, and only then changes the roles to
`LOGIN`.

```zsh
(
  set -euo pipefail
  cd /Users/jungbogeon/starring
  STAGING_DATABASE=starring_runtime_staging
  : "${STARRING_STAGING_CLUSTER_ADMIN:?load the reviewed cluster administrator}"
  : "${STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER:?load the reviewed system identifier}"
  : "${STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT:?load the reviewed dedicated-cluster acknowledgement}"
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
    --file ops/postgres/staging-runtime-role-bootstrap.sql
)
```

Any bootstrap rerun intentionally returns all five roles to `NOLOGIN`, clears
their passwords, and requires a fresh password-and-enable cycle. Stop and
unload the LaunchAgent first. This is credential rotation, not a harmless
read-only check.

## Prove authentication and database isolation

The SCRAM verifier alone does not prove that `pg_hba.conf` requires a
password. For each role, the first probe uses a deliberately invalid password
that cannot satisfy this runbook's password alphabet, the second prompts for
the correct password and invokes that role's readiness function on the target,
and the third prompts again and must be denied on database `postgres`.

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
  CROSS_DATABASE_ERROR="$(mktemp)"
  trap 'rm -f -- "$CROSS_DATABASE_ERROR"' EXIT
  for INDEX in {1..5}; do
    ROLE="${ROLES[$INDEX]}"
    READINESS_FUNCTION="${READINESS_FUNCTIONS[$INDEX]}"
    if PGSSLMODE=disable PGPASSWORD='invalid password probe' \
      /opt/homebrew/opt/postgresql@16/bin/psql \
        --no-psqlrc --set ON_ERROR_STOP=1 \
        --host 127.0.0.1 --port 5432 --username "$ROLE" \
        --dbname "$STAGING_DATABASE" \
        --command 'SELECT 1' >/dev/null 2>&1
    then
      print -u2 -r -- "wrong-password probe unexpectedly succeeded for $ROLE"
      exit 1
    fi
    PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
      --no-psqlrc --set ON_ERROR_STOP=1 --password \
      --host 127.0.0.1 --port 5432 --username "$ROLE" \
      --dbname "$STAGING_DATABASE" \
      --command "SELECT * FROM ${READINESS_FUNCTION}()" >/dev/null
    : >"$CROSS_DATABASE_ERROR"
    if LC_ALL=C PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
      --no-psqlrc --set ON_ERROR_STOP=1 --password \
      --host 127.0.0.1 --port 5432 --username "$ROLE" \
      --dbname postgres --command 'SELECT 1' \
      >/dev/null 2>"$CROSS_DATABASE_ERROR"
    then
      print -u2 -r -- "cross-database probe unexpectedly succeeded for $ROLE"
      exit 1
    fi
    grep -F 'permission denied for database "postgres"' \
      "$CROSS_DATABASE_ERROR" >/dev/null
  done
)
```

The correct passwords are read only by `psql` from the terminal. They are
never command arguments, shell-history text, exported variables, or command
output.

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
  print -r -- "$PID" | grep -Eq '^[0-9]+$'
  LOG_INODE="$(stat -f '%i' "$LOG_PATH")"
  LOG_OFFSET="$(stat -f '%z' "$LOG_PATH")"
  launchctl kill SIGTERM "$SERVICE"
  for ATTEMPT in {1..35}; do
    if ! kill -0 "$PID" 2>/dev/null; then
      break
    fi
    sleep 1
  done
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
  ! print -r -- "$SERVICE_STATE" \
    | grep -Eq '^[[:space:]]*pid = '
  NEW_PID="$(launchctl kickstart -p "$SERVICE")"
  print -r -- "$NEW_PID" | grep -Eq '^[0-9]+$'
  test "$NEW_PID" != "$PID"
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
)
```

The check reads only bytes appended after signaling the captured PID, requires
exactly one clean-shutdown status in that segment, proves launchd did not
restart a failed exit, and starts a distinct PID without `-k`. Treat a deadline
error, an orphaned process, repeated restart, owner loss,
capability-readiness loss, ACK loss, or persistent `not_ready` as a failed
cutover.

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
