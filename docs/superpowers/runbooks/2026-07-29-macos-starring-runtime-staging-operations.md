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

## Preconditions

Operate from the same logged-in macOS account that owns the LaunchAgent and
unlocked login Keychain. Do not run the runtime under `sudo`.

```zsh
cd /Users/jungbogeon/starring
test -z "$(git status --porcelain --untracked-files=normal)"
test "$(git rev-parse --is-inside-work-tree)" = true
test -x /opt/homebrew/opt/postgresql@16/bin/psql
test -x /usr/bin/security
plutil -lint ops/macos/local.starring.runtime.staging.plist
AVAILABLE_KIB="$(df -Pk /Users/jungbogeon | awk 'NR == 2 { print $4 }')"
test "$AVAILABLE_KIB" -ge 31457280
unset AVAILABLE_KIB
```

The disk check enforces at least 30 GiB free on the filesystem containing the
repository. Stop before building, migrating, or rotating credentials if it
fails.

The PostgreSQL role script intentionally removes `PUBLIC` database privileges
cluster-wide so the five runtime roles cannot connect to another database
through inherited `PUBLIC` access. Run it only on the dedicated staging
cluster whose system identifier is explicitly acknowledged. It refuses a
database name that does not contain a standalone `staging` segment.

Apply every repository migration to the target staging database as the common
schema owner before running the role bootstrap. Confirm that the migration
count and latest version match the repository without placing a database
password in a command argument or shell history.

```zsh
STAGING_DATABASE=starring_runtime_staging
REPOSITORY_MIGRATIONS="$(find migrations -type f -name '*.sql' | wc -l | tr -d ' ')"
APPLIED_MIGRATIONS="$(
  /opt/homebrew/opt/postgresql@16/bin/psql \
    --no-psqlrc --dbname "$STAGING_DATABASE" --tuples-only --no-align \
    --command 'SELECT pg_catalog.count(*) FROM public._sqlx_migrations WHERE success'
)"
test "$APPLIED_MIGRATIONS" = "$REPOSITORY_MIGRATIONS"
unset REPOSITORY_MIGRATIONS APPLIED_MIGRATIONS
```

If the migration table is not in `public` on the target installation, inspect
its actual schema as the database owner and make the same exact count check.
Do not continue on a partial or failed migration.

## Bootstrap the five database roles

Capture the non-secret cluster identity, then run the script in bootstrap
mode. The explicit database and cluster acknowledgements prevent applying it
to an unintended PostgreSQL installation.

```zsh
SYSTEM_IDENTIFIER="$(
  /opt/homebrew/opt/postgresql@16/bin/psql \
    --no-psqlrc --dbname postgres --tuples-only --no-align \
    --command 'SELECT system_identifier FROM pg_catalog.pg_control_system()'
)"
test -n "$SYSTEM_IDENTIFIER"
/opt/homebrew/opt/postgresql@16/bin/psql \
  --no-psqlrc --set ON_ERROR_STOP=1 \
  --set runtime_enable=off \
  --set expected_database="$STAGING_DATABASE" \
  --set expected_system_identifier="$SYSTEM_IDENTIFIER" \
  --dbname "$STAGING_DATABASE" \
  --file ops/postgres/staging-runtime-role-bootstrap.sql
```

Bootstrap mode is deliberately fail-closed: it leaves the five roles
`NOLOGIN` with no password. Generate a separate password for every role in a
password manager. Each password must contain 24–512 characters from only
`A-Z`, `a-z`, `0-9`, `_`, `-`, `.`, and `~`; 32 or more random characters is
the operational minimum. In an interactive `psql` session, use the client-side
`\password` command so plaintext is not placed in SQL, server statement logs,
process arguments, or shell history:

```zsh
/opt/homebrew/opt/postgresql@16/bin/psql \
  --no-psqlrc --dbname "$STAGING_DATABASE"
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
/opt/homebrew/opt/postgresql@16/bin/psql \
  --no-psqlrc --set ON_ERROR_STOP=1 \
  --set runtime_enable=on \
  --set expected_database="$STAGING_DATABASE" \
  --set expected_system_identifier="$SYSTEM_IDENTIFIER" \
  --dbname "$STAGING_DATABASE" \
  --file ops/postgres/staging-runtime-role-bootstrap.sql
unset SYSTEM_IDENTIFIER
```

Any bootstrap rerun intentionally returns all five roles to `NOLOGIN`, clears
their passwords, and requires a fresh password-and-enable cycle. This is
credential rotation, not a harmless read-only check.

## Store indirect secrets in Keychain

For a loopback PostgreSQL server, each Keychain value is a complete URL in this
accepted form:

```text
postgresql://ROLE:PASSWORD@127.0.0.1:5432/starring_runtime_staging?sslmode=disable
```

Use the matching distinct role and password for each account below. The
runtime rejects URL encoding, ambient `PG*` connection variables, duplicate
role/database identities, missing explicit ports, and unsafe non-loopback
transport. A remote database requires `sslmode=verify-full` and an absolute
`sslrootcert` path instead.

Run each command exactly as shown with `-w` last. `/usr/bin/security` prompts
for the full URL without putting it in the command line:

```zsh
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
```

The last prompt receives the Discord bot token, not a URL. Never use `-A`,
never place a value after `-w`, and never export these values.

Preflight only the existence of each Keychain item. These commands must not
include `-w`, because `find-generic-password -w` prints the secret:

```zsh
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
unset ACCOUNT
```

## Build and install an immutable revision

The executable rejects a build without its exact 40-character Git revision
compiled in. Build from a clean commit and retain the previous immutable
binary for rollback.

```zsh
cd /Users/jungbogeon/starring
test -z "$(git status --porcelain --untracked-files=normal)"
REVISION="$(git rev-parse HEAD)"
printf '%s\n' "$REVISION" | grep -Eq '^[0-9a-f]{40}$'
STARRING_RUNTIME_BUILD_REVISION="$REVISION" \
  cargo build --locked --release -p starring-runtime
mkdir -p "$HOME/.local/libexec" "$HOME/Library/Logs/starring-runtime"
chmod 700 "$HOME/.local/libexec" "$HOME/Library/Logs/starring-runtime"
install -m 500 target/release/starring-runtime \
  "$HOME/.local/libexec/starring-runtime-$REVISION"
PREVIOUS_BINARY="$(readlink "$HOME/.local/libexec/starring-runtime" 2>/dev/null || true)"
ln -sfn "starring-runtime-$REVISION" \
  "$HOME/.local/libexec/starring-runtime"
test -x "$HOME/.local/libexec/starring-runtime"
```

Keep `PREVIOUS_BINARY` in the operator's current shell until cutover is
accepted. It is a path, not a secret.

## Install and start the LaunchAgent

```zsh
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
launchctl bootout "$SERVICE" 2>/dev/null || true
launchctl enable "$SERVICE"
launchctl bootstrap "$DOMAIN" "$INSTALLED_PLIST"
launchctl kickstart -k "$SERVICE"
```

`KeepAlive.SuccessfulExit=false` restarts a failed process but leaves an
intentional clean SIGTERM shutdown stopped. A 30-second throttle prevents a
tight crash loop.

## Health and empty-open verification

First prove the listener is loopback-only and liveness is available:

```zsh
lsof -nP -iTCP:19091 -sTCP:LISTEN
curl --fail --silent --show-error --max-time 1 \
  http://127.0.0.1:19091/health/live
```

The listener output must name only `127.0.0.1:19091`. Stop immediately if it
shows `*:19091`, `[::]:19091`, or any non-loopback address.

Readiness remains `503 not_ready` while recovery, exact capability checks,
paused Discord connection, owner acquisition, and the durable ingress-open
acknowledgement converge. Allow one bounded startup window:

```zsh
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
unset READY ATTEMPT
launchctl print "$SERVICE"
tail -n 100 "$HOME/Library/Logs/starring-runtime/runtime.log"
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
PID="$(
  launchctl print "$SERVICE" \
    | awk '/^[[:space:]]*pid = / { print $3; exit }'
)"
test -n "$PID"
launchctl kill SIGTERM "$SERVICE"
for ATTEMPT in {1..35}; do
  if ! kill -0 "$PID" 2>/dev/null; then
    break
  fi
  sleep 1
done
! kill -0 "$PID" 2>/dev/null
tail -n 20 "$HOME/Library/Logs/starring-runtime/runtime.log" \
  | grep -F 'starring_runtime_status=runtime_process_clean_shutdown'
launchctl kickstart -k "$SERVICE"
unset PID ATTEMPT
```

After restart, repeat both health checks and confirm a new PID reaches
`ready`. Treat a deadline error, an orphaned process, repeated restart, owner
loss, capability-readiness loss, ACK loss, or persistent `not_ready` as a
failed cutover.

## Routine operation

```zsh
DOMAIN="gui/$(id -u)"
SERVICE="$DOMAIN/local.starring.runtime.staging"
launchctl print "$SERVICE"
curl --fail --silent --show-error --max-time 1 \
  http://127.0.0.1:19091/health/live
curl --fail --silent --show-error --max-time 1 \
  http://127.0.0.1:19091/health/ready
tail -n 100 "$HOME/Library/Logs/starring-runtime/runtime.log"
```

Do not increase pool sizes, channel capacities, owner lease timings, or drain
timeouts without a new measured SLO cohort. Do not expose port `19091` through
Cloudflare, a router, a public DNS record, or a non-loopback proxy.

## Stop and rollback

An intentional stop uses launchd so the process receives SIGTERM and the
35-second launchd timeout remains outside the runtime's 30-second deadline:

```zsh
DOMAIN="gui/$(id -u)"
SERVICE="$DOMAIN/local.starring.runtime.staging"
launchctl bootout "$SERVICE"
```

To roll back the executable, keep the service stopped, restore the previous
immutable target and optional previous plist, validate the plist, then start
it:

```zsh
test -n "$PREVIOUS_BINARY"
test -x "$HOME/.local/libexec/$PREVIOUS_BINARY"
ln -sfn "$PREVIOUS_BINARY" "$HOME/.local/libexec/starring-runtime"
INSTALLED_PLIST="$HOME/Library/LaunchAgents/local.starring.runtime.staging.plist"
if test -f "$INSTALLED_PLIST.previous"; then
  install -m 600 "$INSTALLED_PLIST.previous" "$INSTALLED_PLIST"
fi
plutil -lint "$INSTALLED_PLIST"
launchctl bootstrap "$DOMAIN" "$INSTALLED_PLIST"
launchctl kickstart -k "$SERVICE"
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
