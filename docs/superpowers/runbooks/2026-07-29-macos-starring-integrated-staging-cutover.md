# macOS Starring Integrated Staging Cutover

## Status: not executed

This runbook is an executable staging plan. No command in this document was run
as part of authoring it. No old cluster was stopped or moved, no new cluster was
initialized, no role or Keychain item was changed, and no LaunchAgent or tunnel
was loaded or unloaded. Every acceptance result and receipt remains absent
until an operator runs the applicable block and records its exit status.

This cutover creates a disposable, dedicated PostgreSQL 16 staging cluster for
the Starring API and the empty-open runtime on one Mac mini. It does not
authorize customer traffic, a customer Discord guild, or production Live
automation. Runtime readiness in this release means paused Discord connection,
owner and ingress-acknowledgement convergence, and an empty in-process registry.
It does not mean that a customer interaction route is installed.

The host still uses one macOS GUI login as the Keychain and LaunchAgent
boundary. Codex and other processes under that login are inside the same secret
threat boundary. All credentials provisioned here must therefore be disposable
staging credentials.

## Normative sources and override rules

This document orders the complete cutover and deliberately reuses the audited
blocks in:

- [Production Control Plane Cutover Runbook](./2026-07-19-production-control-plane-cutover.md)
- [macOS Starring Runtime Staging Operations](./2026-07-29-macos-starring-runtime-staging-operations.md)
- `ops/postgres/staging-api-role-bootstrap.sql`
- `ops/postgres/staging-api-role-enable.sql`
- `ops/postgres/staging-runtime-role-bootstrap.sql`
- `ops/postgres/staging-integrated-bootstrap-pg_hba.conf`
- `ops/postgres/staging-integrated-bootstrap-pg_ident.conf`
- `ops/postgres/staging-integrated-pg_hba.conf`
- `tools/starring-db-bootstrap`

The fixed database name in this runbook is
`starring_runtime_staging`. It replaces the older illustrative
`starring_staging` name wherever an API block is reused. The temporary
bootstrap HBA and ident manifests are allowed only while both applications and
public ingress are stopped. The final integrated HBA replaces the runtime-only
HBA block in the runtime runbook. Do not run the runtime-only four-rule
installer or synthesize either integrated manifest in the operator shell.

The SQL manifests, API binary installation block, runtime immutable-build
block, runtime SIGTERM acceptance block, and component readiness contracts
remain normative. Do not copy a fragment out of a SQL manifest or replace a
manifest with the explanatory grant snippets in either source runbook.

## Fixed contract

| Item | Exact value |
| --- | --- |
| repository | `/Users/jungbogeon/starring` |
| PostgreSQL formula | `postgresql@16` |
| active PGDATA | `/opt/homebrew/var/postgresql@16` |
| database | `starring_runtime_staging` |
| database host | `127.0.0.1` |
| database port | `5432` |
| owner | `starring_owner`, `NOLOGIN` |
| cluster administrator | exactly `starring_cluster_admin` |
| bootstrap OS user | exactly `jungbogeon` |
| bootstrap socket | `/private/tmp/starring-bootstrap` |
| API label | `local.starring.api.staging` |
| runtime label | `local.starring.runtime.staging` |
| Starring tunnel label | `local.cloudflared.starring` |
| API listener | `127.0.0.1:18080` |
| runtime listener | `127.0.0.1:19091` |
| API Keychain service | `starring-api.staging` |
| runtime Keychain service | `starring.runtime.staging` |
| PostgreSQL major | exactly `16` |
| page checksums | enabled |
| password encryption and HBA | `scram-sha-256` |

Do not substitute the currently running `com.remine.web-tunnel` service for
`local.cloudflared.starring`. It is a separate workload. A missing Starring
tunnel is recorded as not previously loaded; another tunnel is never stopped
by this procedure.

## Credential inventory

There are exactly twenty application database login credentials: fifteen for
the API and five for the runtime. They all connect to the one fixed
database, use distinct role names and passwords, and have no membership.

| Keychain service/account | Exact database role |
| --- | --- |
| `starring-api.staging/database.oauth-flow-writer` | `starring_identity_oauth` |
| `starring-api.staging/database.session-issuer` | `starring_identity_issuer` |
| `starring-api.staging/database.session-api` | `starring_identity_session` |
| `starring-api.staging/database.security-revoker` | `starring_identity_security` |
| `starring-api.staging/database.installation-authority-reader` | `starring_installation_authority_reader` |
| `starring-api.staging/database.authorized-snapshot-reader` | `starring_authorized_snapshot_reader` |
| `starring-api.staging/database.promotion-executor` | `starring_promotion_executor` |
| `starring-api.staging/database.decision-reader` | `starring_decision_reader` |
| `starring-api.staging/database.approval-executor` | `starring_decision_approval` |
| `starring-api.staging/database.rejection-executor` | `starring_decision_rejection` |
| `starring-api.staging/database.apply-executor` | `starring_decision_apply` |
| `starring-api.staging/database.cancellation-executor` | `starring_decision_cancellation` |
| `starring-api.staging/database.deployment-status-reader` | `starring_deployment_status_reader` |
| `starring-api.staging/database.operational-deployment-status-reader` | `starring_operational_deployment_status_reader` |
| `starring-api.staging/database.authoring-session-writer` | `starring_authoring_session_writer` |
| `starring.runtime.staging/database.execution` | `starring_runtime_execution` |
| `starring.runtime.staging/database.exact-target` | `starring_runtime_exact_target` |
| `starring.runtime.staging/database.panel` | `starring_runtime_panel` |
| `starring.runtime.staging/database.serving` | `starring_runtime_serving` |
| `starring.runtime.staging/database.interaction` | `starring_runtime_interaction` |

Each database item is a complete URL in this exact shape:

```text
postgresql://ROLE:PASSWORD@127.0.0.1:5432/starring_runtime_staging?sslmode=disable
```

The provisioner generates each application password independently from 32
random bytes and encodes it as exactly 43 URL-safe unpadded Base64 characters.
Operators never type, export, or record these generated values.

There are three Discord provider-credential Keychain entries in addition to
the twenty application database entries:

| Keychain service/account | Value |
| --- | --- |
| `starring-api.staging/discord.oauth-client-secret` | Discord OAuth client secret |
| `starring-api.staging/discord.bot-token` | Discord bot token |
| `starring.runtime.staging/discord.bot-token` | the same reviewed bot token for the runtime |

These three provider entries must already exist and remain unchanged. The two
bot-token entries must contain the same reviewed token.

Conversational authoring also requires one pre-existing private loopback worker
credential:

| Keychain service/account | Value |
| --- | --- |
| `com.starring.llm-api-key/llm-api` | Codex worker bearer credential |

The provisioner does not create, rotate, or read this item. Gate 0 proves only
that it exists, and the installed API plist references it by fixed Keychain
identity. No runbook command reads or prints its value.

There are exactly two API keyring entries:

| Keychain service/account | Purpose |
| --- | --- |
| `starring-api.staging/keyring.product-action` | product-action digest |
| `starring-api.staging/keyring.snapshot-envelope` | authorized snapshot encryption |

Each keyring is one compact version-1 JSON object with one active key and zero
through seven retired keys. Every material value is canonical Base64 for
exactly 32 random bytes. Key IDs are immutable and unique inside a keyring, and
material must differ across the two purposes. Record key IDs only. Never record
material or a material hash.

There is exactly one runtime interaction-token envelope keyring entry:

| Keychain service/account | Purpose |
| --- | --- |
| `starring.runtime.staging/interaction.token-envelope-keyring` | restart-recoverable Discord interaction-token encryption |

Its payload is exactly
`v1;active=KEY_ID=64_LOWERCASE_HEX;retired=KEY_ID=64_LOWERCASE_HEX,...`.
It contains one active key and zero through seven retired keys, for at most
eight keys total. Each material value is exactly 32 independently generated
random bytes encoded as 64 lowercase hexadecimal characters. IDs are bounded,
unique, and immutable within the keyring. Materials are unique within the
keyring and across both API keyring purposes. Record only the active key ID.
Never record material or a material hash.

The provisioner also creates one operational administrator URL:

| Keychain service/account | Exact database role and database |
| --- | --- |
| `starring.postgres.staging/database.cluster-admin` | `starring_cluster_admin` on `postgres` |

The complete inventory after provisioning is twenty-one database URLs, three
unchanged Discord provider items, one unchanged authoring-worker item, two API
keyrings, and one runtime interaction-token keyring: twenty-eight Keychain
items. The application plists reference only their own twenty URLs, the three
Discord provider items, the authoring-worker item, the two API keyrings, and
the runtime interaction-token keyring. They never reference the administrator
URL.

The cluster-administrator password is generated only by the one-shot
provisioner and is never typed or exported. `starring_owner` has no password.
The embedded bootstrap does not create `starring_migrator`, does not accept a
database URL, and removes every inbound or outbound owner membership before it
returns.

## Gate 0: approved inputs and operator shell

Use one dedicated `zsh` for the whole maintenance window. The following values
are non-secret. Set them from an independently reviewed change record. Do not
derive the approved revision, administrator, public origin, Discord IDs, or
tunnel identity from the target during the cutover.

```zsh
set -euo pipefail
set +x
export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"
export STARRING_CUTOVER_ID="REPLACE_WITH_UTC_CUTOVER_ID"
export STARRING_APPROVED_RELEASE_REVISION="REPLACE_WITH_40_HEX_REVISION"
export STARRING_STAGING_CLUSTER_ADMIN="starring_cluster_admin"
export STARRING_STAGING_PUBLIC_ORIGIN="https://REPLACE_WITH_STAGING_HOST"
export STARRING_STAGING_PUBLIC_HOST="REPLACE_WITH_STAGING_HOST"
export STARRING_DISCORD_APPLICATION_ID="REPLACE_WITH_APPLICATION_ID"
export STARRING_DISCORD_BOT_USER_ID="REPLACE_WITH_BOT_USER_ID"
export STARRING_STAGING_TUNNEL_LABEL="local.cloudflared.starring"
export STARRING_PGDATA="/opt/homebrew/var/postgresql@16"
export STARRING_OLD_PGDATA_ARCHIVE="/opt/homebrew/var/postgresql@16.pre-starring-${STARRING_CUTOVER_ID}"
export STARRING_CUTOVER_EVIDENCE="$HOME/Library/Application Support/Starring/cutovers/${STARRING_CUTOVER_ID}"
export STARRING_BOOTSTRAP_SOCKET_DIR="/private/tmp/starring-bootstrap"
export STARRING_BOOTSTRAP_BINARY="$HOME/.local/libexec/starring-db-bootstrap-${STARRING_APPROVED_RELEASE_REVISION}"
export STARRING_PROVISIONER_BINARY="$HOME/.local/libexec/starring-staging-provisioner-${STARRING_APPROVED_RELEASE_REVISION}"
starring_admin_pgpass() {
  /usr/bin/security find-generic-password -w \
    -s starring.postgres.staging \
    -a database.cluster-admin \
    | /usr/bin/sed -nE \
      's#^postgresql://starring_cluster_admin:([A-Za-z0-9_-]{43})@127\.0\.0\.1:5432/postgres\?sslmode=disable$#127.0.0.1:5432:*:starring_cluster_admin:\1#p'
}
starring_with_admin_pgpass() (
  set +x
  umask 077
  ADMIN_PGPASS_DIR=
  ADMIN_PGPASS_PATH=
  COMMAND_STATUS=0
  CLEANUP_STATUS=0
  starring_cleanup_admin_pgpass() {
    CLEANUP_RESULT=0
    if test -n "${ADMIN_PGPASS_PATH:-}" \
      && test -e "$ADMIN_PGPASS_PATH"
    then
      if test -f "$ADMIN_PGPASS_PATH" \
        && ! test -L "$ADMIN_PGPASS_PATH"
      then
        /bin/dd if=/dev/zero of="$ADMIN_PGPASS_PATH" \
          bs=4096 count=1 conv=notrunc >/dev/null 2>&1 \
          || CLEANUP_RESULT=1
      else
        CLEANUP_RESULT=1
      fi
      /bin/rm -f "$ADMIN_PGPASS_PATH" >/dev/null 2>&1 \
        || CLEANUP_RESULT=1
    fi
    if test -n "${ADMIN_PGPASS_DIR:-}"
    then
      /bin/rmdir "$ADMIN_PGPASS_DIR" >/dev/null 2>&1 \
        || CLEANUP_RESULT=1
    fi
    return "$CLEANUP_RESULT"
  }
  trap \
    'TRAP_STATUS=$?; trap - EXIT HUP INT TERM; starring_cleanup_admin_pgpass || CLEANUP_STATUS=$?; if test "$TRAP_STATUS" -eq 0 && test "$CLEANUP_STATUS" -ne 0; then exit "$CLEANUP_STATUS"; fi; exit "$TRAP_STATUS"' \
    EXIT
  trap 'exit 129' HUP
  trap 'exit 130' INT
  trap 'exit 143' TERM
  if ! ADMIN_PGPASS_DIR="$(
    /usr/bin/mktemp -d /private/tmp/starring-admin-pgpass.XXXXXX
  )"
  then
    return 1
  fi
  if ! test -d "$ADMIN_PGPASS_DIR" \
    || ! test "$(
      /usr/bin/stat -f '%u:%Lp' "$ADMIN_PGPASS_DIR"
    )" = "$(/usr/bin/id -u):700" \
    || ! /bin/ls -lde "$ADMIN_PGPASS_DIR" \
      | /usr/bin/awk \
        'NR == 1 { ok = ($1 == "drwx------" || $1 == "drwx------@") } NR > 1 { ok = 0 } END { exit(ok ? 0 : 1) }'
  then
    return 1
  fi
  ADMIN_PGPASS_PATH="$ADMIN_PGPASS_DIR/pgpass"
  if ! starring_admin_pgpass >"$ADMIN_PGPASS_PATH"
  then
    COMMAND_STATUS=1
  elif ! test -f "$ADMIN_PGPASS_PATH" \
    || test -L "$ADMIN_PGPASS_PATH" \
    || ! test "$(
      /usr/bin/stat -f '%u:%Lp' "$ADMIN_PGPASS_PATH"
    )" = "$(/usr/bin/id -u):600" \
    || ! /bin/ls -le "$ADMIN_PGPASS_PATH" \
      | /usr/bin/awk \
        'NR == 1 { ok = ($1 == "-rw-------" || $1 == "-rw-------@") } NR > 1 { ok = 0 } END { exit(ok ? 0 : 1) }' \
    || ! test "$(
      /usr/bin/wc -l <"$ADMIN_PGPASS_PATH" | /usr/bin/tr -d ' '
    )" = 1 \
    || ! /usr/bin/grep -Eq \
      '^127\.0\.0\.1:5432:\*:starring_cluster_admin:[A-Za-z0-9_-]{43}$' \
      "$ADMIN_PGPASS_PATH"
  then
    COMMAND_STATUS=1
  else
    PGPASSFILE="$ADMIN_PGPASS_PATH" "$@" || COMMAND_STATUS=$?
  fi
  return "$COMMAND_STATUS"
)
```

`STARRING_CUTOVER_ID` must match
`^[0-9]{8}T[0-9]{6}Z$`. The public host must be the origin with only the
`https://` prefix removed. The cluster administrator, bootstrap operating
system user, socket path, database, and both application listeners are fixed.

Run the local preflight before stopping anything:

```zsh
(
  set -euo pipefail
  set +x
  cd /Users/jungbogeon/starring
  : "${STARRING_CUTOVER_ID:?}"
  : "${STARRING_APPROVED_RELEASE_REVISION:?}"
  : "${STARRING_STAGING_CLUSTER_ADMIN:?}"
  : "${STARRING_STAGING_PUBLIC_ORIGIN:?}"
  : "${STARRING_STAGING_PUBLIC_HOST:?}"
  : "${STARRING_DISCORD_APPLICATION_ID:?}"
  : "${STARRING_DISCORD_BOT_USER_ID:?}"
  : "${STARRING_STAGING_TUNNEL_LABEL:?}"
  : "${STARRING_PGDATA:?}"
  : "${STARRING_OLD_PGDATA_ARCHIVE:?}"
  : "${STARRING_CUTOVER_EVIDENCE:?}"
  print -r -- "$STARRING_CUTOVER_ID" \
    | grep -Eq '^[0-9]{8}T[0-9]{6}Z$'
  print -r -- "$STARRING_APPROVED_RELEASE_REVISION" \
    | grep -Eq '^[0-9a-f]{40}$'
  test "$STARRING_STAGING_CLUSTER_ADMIN" = starring_cluster_admin
  test "$(id -un)" = jungbogeon
  print -r -- "$STARRING_DISCORD_APPLICATION_ID" \
    | grep -Eq '^[1-9][0-9]{0,19}$'
  print -r -- "$STARRING_DISCORD_BOT_USER_ID" \
    | grep -Eq '^[1-9][0-9]{0,19}$'
  test "$STARRING_STAGING_PUBLIC_ORIGIN" \
    = "https://${STARRING_STAGING_PUBLIC_HOST}"
  print -r -- "$STARRING_STAGING_PUBLIC_HOST" \
    | grep -Eq '^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?)+$'
  test "$STARRING_STAGING_TUNNEL_LABEL" = local.cloudflared.starring
  test "$STARRING_PGDATA" = /opt/homebrew/var/postgresql@16
  test "$STARRING_BOOTSTRAP_SOCKET_DIR" \
    = /private/tmp/starring-bootstrap
  test "$STARRING_BOOTSTRAP_BINARY" \
    = "$HOME/.local/libexec/starring-db-bootstrap-${STARRING_APPROVED_RELEASE_REVISION}"
  test "$STARRING_PROVISIONER_BINARY" \
    = "$HOME/.local/libexec/starring-staging-provisioner-${STARRING_APPROVED_RELEASE_REVISION}"
  test "$STARRING_OLD_PGDATA_ARCHIVE" \
    = "${STARRING_PGDATA}.pre-starring-${STARRING_CUTOVER_ID}"
  test -z "$(git status --porcelain --untracked-files=normal)"
  test "$(git rev-parse HEAD)" = "$STARRING_APPROVED_RELEASE_REVISION"
  git merge-base --is-ancestor \
    "$STARRING_APPROVED_RELEASE_REVISION" origin/main
  test "$(brew list --versions postgresql@16 | awk '{print $2}')" \
    = 16.14
  test -x /opt/homebrew/opt/postgresql@16/bin/initdb
  test -x /opt/homebrew/opt/postgresql@16/bin/pg_checksums
  test -x /opt/homebrew/opt/postgresql@16/bin/pg_controldata
  test -x /opt/homebrew/opt/postgresql@16/bin/pg_ctl
  test -x /opt/homebrew/opt/postgresql@16/bin/psql
  test -x /usr/bin/security
  test -x "$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin/cargo"
  test -x "$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc"
  test -f ops/postgres/staging-integrated-bootstrap-pg_hba.conf
  test -f ops/postgres/staging-integrated-bootstrap-pg_ident.conf
  test -f ops/postgres/staging-integrated-pg_hba.conf
  test -d tools/starring-db-bootstrap
  test -d tools/starring-staging-provisioner
  for ENTRY in \
    "starring-api.staging:discord.oauth-client-secret" \
    "starring-api.staging:discord.bot-token" \
    "starring.runtime.staging:discord.bot-token" \
    "com.starring.llm-api-key:llm-api"
  do
    SERVICE="${ENTRY%%:*}"
    ACCOUNT="${ENTRY#*:}"
    /usr/bin/security find-generic-password \
      -s "$SERVICE" -a "$ACCOUNT" >/dev/null
  done
  plutil -lint ops/macos/local.starring.api.staging.plist
  plutil -lint ops/macos/local.starring.runtime.staging.plist
  test "$(df -Pk /Users/jungbogeon | awk 'NR == 2 { print $4 }')" \
    -ge 10485760
  test ! -e "$STARRING_OLD_PGDATA_ARCHIVE"
  test ! -e "$STARRING_CUTOVER_EVIDENCE"
)
```

Build and test the bootstrap binary from the exact approved revision before
the outage. Its migrations are embedded at compile time. The immutable copy is
the only bootstrap binary permitted during the cutover:

```zsh
(
  set -euo pipefail
  set +x
  cd /Users/jungbogeon/starring
  test -z "$(git status --porcelain --untracked-files=normal)"
  test "$(git rev-parse HEAD)" = "$STARRING_APPROVED_RELEASE_REVISION"
  cargo test --locked -p starring-db-bootstrap \
    -p starring-staging-provisioner
  cargo build --locked --release \
    -p starring-db-bootstrap \
    -p starring-staging-provisioner
  mkdir -p "$HOME/.local/libexec"
  install -m 500 target/release/starring-db-bootstrap \
    "$STARRING_BOOTSTRAP_BINARY"
  install -m 500 target/release/starring-staging-provisioner \
    "$STARRING_PROVISIONER_BINARY"
  test -x "$STARRING_BOOTSTRAP_BINARY"
  test -x "$STARRING_PROVISIONER_BINARY"
)
```

This integrated cutover requires at least 10 GiB free. The old physical
cluster is retained by a same-volume rename rather than copied, while the new
cluster, release builds, logs, and rollback evidence consume additional
space. A lower value stops the cutover.

Before proceeding, the change record must also contain:

- confirmation that all existing values for any Keychain account that will be
  updated remain recoverable from an external password manager;
- an approved API public origin and matching Discord callback;
- an approved PostgreSQL 16.14 and Rust locked toolchain;
- separately reviewed bootstrap HBA, ident, and final HBA manifests matching
  Gates 5 and 10A;
- an owner for the Cloudflare edge configuration;
- a rollback decision-maker and maintenance deadline.

## Gate 1: record entry state and unload ingress and clients

This gate creates non-secret evidence, archives existing installed service
artifacts, records whether each exact service was loaded, and then unloads only
those exact services. It never unloads `com.remine.web-tunnel`.

```zsh
(
  set -euo pipefail
  set +x
  umask 077
  mkdir -p "$STARRING_CUTOVER_EVIDENCE/pre-cutover"
  DOMAIN="gui/$(id -u)"
  ENTRY_STATE="$STARRING_CUTOVER_EVIDENCE/entry-state.env"
  test ! -e "$ENTRY_STATE"
  for ENTRY in \
    "tunnel:${STARRING_STAGING_TUNNEL_LABEL}" \
    "api:local.starring.api.staging" \
    "runtime:local.starring.runtime.staging"
  do
    KEY="${ENTRY%%:*}"
    LABEL="${ENTRY#*:}"
    SERVICE="$DOMAIN/$LABEL"
    if launchctl print "$SERVICE" >/dev/null 2>&1
    then
      print -r -- "${KEY}_was_loaded=true" >>"$ENTRY_STATE"
      launchctl bootout "$SERVICE"
    else
      print -r -- "${KEY}_was_loaded=false" >>"$ENTRY_STATE"
    fi
    ! launchctl print "$SERVICE" >/dev/null 2>&1
  done
  PG_STATUS="$(
    brew services list \
      | awk '$1 == "postgresql@16" { print $2 }'
  )"
  if test "$PG_STATUS" = started
  then
    print -r -- 'postgresql_was_started=true' >>"$ENTRY_STATE"
  else
    print -r -- 'postgresql_was_started=false' >>"$ENTRY_STATE"
  fi
  for ARTIFACT in \
    "$HOME/Library/LaunchAgents/local.starring.api.staging.plist" \
    "$HOME/Library/LaunchAgents/local.starring.runtime.staging.plist" \
    "$HOME/Library/LaunchAgents/local.cloudflared.starring.plist" \
    "$HOME/.local/libexec/starring-api"
  do
    if test -f "$ARTIFACT"
    then
      cp -p "$ARTIFACT" \
        "$STARRING_CUTOVER_EVIDENCE/pre-cutover/$(basename "$ARTIFACT")"
      shasum -a 256 "$ARTIFACT" \
        >>"$STARRING_CUTOVER_EVIDENCE/pre-cutover/artifact-sha256.txt"
    fi
  done
  if test -L "$HOME/.local/libexec/starring-runtime"
  then
    readlink "$HOME/.local/libexec/starring-runtime" \
      >"$STARRING_CUTOVER_EVIDENCE/pre-cutover/runtime-link-target.txt"
  fi
  brew services list \
    >"$STARRING_CUTOVER_EVIDENCE/pre-cutover/brew-services.txt"
  brew services stop postgresql@16
  for ATTEMPT in {1..60}
  do
    if ! lsof -nP -iTCP:5432 -sTCP:LISTEN >/dev/null 2>&1
    then
      break
    fi
    sleep 1
  done
  ! lsof -nP -iTCP:5432 -sTCP:LISTEN >/dev/null 2>&1
  ! lsof -nP -iTCP:18080 -sTCP:LISTEN >/dev/null 2>&1
  ! lsof -nP -iTCP:19091 -sTCP:LISTEN >/dev/null 2>&1
)
```

Stop every ad-hoc `psql`, SQLx migration, test, scheduler, and database pool
before Gate 2. A process outside launchd is not made safe by unloading the
three labels.

## Gate 2: inventory and archive the old cluster without deletion

The old PGDATA becomes the rollback archive by an atomic same-filesystem
rename. It is not deleted, truncated, upgraded, or reused. The new cluster will
later take the old fixed path so the Homebrew LaunchAgent remains unmodified.

```zsh
(
  set -euo pipefail
  set +x
  test -d "$STARRING_PGDATA"
  test ! -e "$STARRING_OLD_PGDATA_ARCHIVE"
  ! /opt/homebrew/opt/postgresql@16/bin/pg_ctl \
    --pgdata "$STARRING_PGDATA" status >/dev/null 2>&1
  test ! -e "$STARRING_PGDATA/postmaster.pid"
  LC_ALL=C /opt/homebrew/opt/postgresql@16/bin/pg_controldata \
    "$STARRING_PGDATA" \
    >"$STARRING_CUTOVER_EVIDENCE/pre-cutover/old-pg-controldata.txt"
  grep -Eq '^Database cluster state:[[:space:]]+shut down$' \
    "$STARRING_CUTOVER_EVIDENCE/pre-cutover/old-pg-controldata.txt"
  test "$(tr -d '[:space:]' <"$STARRING_PGDATA/PG_VERSION")" = 16
  du -sk "$STARRING_PGDATA" \
    >"$STARRING_CUTOVER_EVIDENCE/pre-cutover/old-pgdata-kib.txt"
  find "$STARRING_PGDATA" -xdev -type f \
    | wc -l \
    >"$STARRING_CUTOVER_EVIDENCE/pre-cutover/old-pgdata-file-count.txt"
  for CONFIG in postgresql.conf pg_hba.conf pg_ident.conf
  do
    test -f "$STARRING_PGDATA/$CONFIG"
    shasum -a 256 "$STARRING_PGDATA/$CONFIG" \
      >>"$STARRING_CUTOVER_EVIDENCE/pre-cutover/old-config-sha256.txt"
  done
  test "$(stat -f '%d' "$STARRING_PGDATA")" \
    = "$(stat -f '%d' "$(dirname "$STARRING_OLD_PGDATA_ARCHIVE")")"
  sync
  mv "$STARRING_PGDATA" "$STARRING_OLD_PGDATA_ARCHIVE"
  sync
  test ! -e "$STARRING_PGDATA"
  test -d "$STARRING_OLD_PGDATA_ARCHIVE"
  test "$(tr -d '[:space:]' \
    <"$STARRING_OLD_PGDATA_ARCHIVE/PG_VERSION")" = 16
  print -r -- "$STARRING_OLD_PGDATA_ARCHIVE" \
    >"$STARRING_CUTOVER_EVIDENCE/old-pgdata-archive-path.txt"
)
```

If the clean-shutdown proof or rename fails, do not initialize a new cluster.
Restore only the service state appropriate to the still-active old path and
end the maintenance window.

## Gate 3: initialize a fresh PostgreSQL 16 cluster

`initdb` creates the fixed administrator without a password. The temporary
peer boundary is the only initial access path. Gate 10 later generates its
administrator credential without a prompt; it is never put in a shell
variable, `--pwfile`, process argument, evidence file, or application
Keychain item.

```zsh
(
  set -euo pipefail
  set +x
  test ! -e "$STARRING_PGDATA"
  test ! -e "$STARRING_BOOTSTRAP_SOCKET_DIR"
  mkdir -m 700 "$STARRING_BOOTSTRAP_SOCKET_DIR"
  chgrp "$(id -gn)" "$STARRING_BOOTSTRAP_SOCKET_DIR"
  test "$(stat -f '%Su:%Sg:%Lp' "$STARRING_BOOTSTRAP_SOCKET_DIR")" \
    = "$(id -un):$(id -gn):700"
  LC_ALL=C /opt/homebrew/opt/postgresql@16/bin/initdb \
    --pgdata "$STARRING_PGDATA" \
    --encoding=UTF8 \
    --locale=C \
    --data-checksums \
    --auth-local=peer \
    --auth-host=reject \
    --username "$STARRING_STAGING_CLUSTER_ADMIN" \
    --set=listen_addresses=127.0.0.1 \
    --set=unix_socket_directories=/private/tmp/starring-bootstrap \
    --set=port=5432 \
    --set=password_encryption=scram-sha-256 \
    --set=ssl=off
  test "$(tr -d '[:space:]' <"$STARRING_PGDATA/PG_VERSION")" = 16
  test "$(stat -f '%Lp' "$STARRING_PGDATA")" = 700
  LC_ALL=C /opt/homebrew/opt/postgresql@16/bin/pg_checksums \
    --check --pgdata "$STARRING_PGDATA" \
    >"$STARRING_CUTOVER_EVIDENCE/new-pg-checksums.txt"
  LC_ALL=C /opt/homebrew/opt/postgresql@16/bin/pg_controldata \
    "$STARRING_PGDATA" \
    >"$STARRING_CUTOVER_EVIDENCE/new-pg-controldata.txt"
  grep -Eq '^Database cluster state:[[:space:]]+shut down$' \
    "$STARRING_CUTOVER_EVIDENCE/new-pg-controldata.txt"
  grep -Eq '^Data page checksum version:[[:space:]]+1$' \
    "$STARRING_CUTOVER_EVIDENCE/new-pg-controldata.txt"
)
```

## Gate 4: offline system identifier and v2 acknowledgement

Read the new identifier while the server is still offline and record it in the
change system. A reviewer who did not derive an expected value from a live
connection must approve the identifier and construct the v2 acknowledgement.
This is a mandatory pause.

```zsh
(
  set -euo pipefail
  set +x
  OFFLINE_SYSTEM_IDENTIFIER="$(
    LC_ALL=C /opt/homebrew/opt/postgresql@16/bin/pg_controldata \
      "$STARRING_PGDATA" \
      | awk -F: '
        /^Database system identifier:/ {
          value=$2
          gsub(/[[:space:]]/, "", value)
          print value
        }
      '
  )"
  print -r -- "$OFFLINE_SYSTEM_IDENTIFIER" | grep -Eq '^[0-9]+$'
  print -r -- "$OFFLINE_SYSTEM_IDENTIFIER" \
    >"$STARRING_CUTOVER_EVIDENCE/offline-system-identifier.txt"
  print -r -- "$OFFLINE_SYSTEM_IDENTIFIER"
)
```

After the independent review, set these non-secret values in the operator
shell:

```zsh
export STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER="REPLACE_WITH_REVIEWED_IDENTIFIER"
export STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT="starring-runtime-dedicated-staging-cluster-v2:${STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER}:starring_runtime_staging:cluster-wide-public-acl-reset:bidirectional-runtime-membership-revocation"
```

Verify the offline gate without starting PostgreSQL:

```zsh
(
  set -euo pipefail
  set +x
  : "${STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER:?}"
  : "${STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT:?}"
  test "$(
    tr -d '[:space:]' \
      <"$STARRING_CUTOVER_EVIDENCE/offline-system-identifier.txt"
  )" = "$STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER"
  EXPECTED_ACKNOWLEDGEMENT="starring-runtime-dedicated-staging-cluster-v2:${STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER}:starring_runtime_staging:cluster-wide-public-acl-reset:bidirectional-runtime-membership-revocation"
  test "$STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT" \
    = "$EXPECTED_ACKNOWLEDGEMENT"
)
```

## Gate 5: install the temporary peer bootstrap boundary

This is the only gate that installs the temporary peer map. The reviewed HBA
permits exactly the `jungbogeon` operating-system user to become
`starring_cluster_admin` through the private Unix socket for only `postgres`
and `starring_runtime_staging`. Every TCP, other socket, and physical
replication path is rejected. Both applications and public ingress remain
stopped until the final HBA replaces this file.

```zsh
(
  set -euo pipefail
  set +x
  cd /Users/jungbogeon/starring
  test "$STARRING_STAGING_CLUSTER_ADMIN" = starring_cluster_admin
  test "$(id -un)" = jungbogeon
  test "$(wc -l <ops/postgres/staging-integrated-bootstrap-pg_hba.conf | tr -d ' ')" = 7
  test "$(wc -l <ops/postgres/staging-integrated-bootstrap-pg_ident.conf | tr -d ' ')" = 1
  cp -p "$STARRING_PGDATA/pg_hba.conf" \
    "$STARRING_CUTOVER_EVIDENCE/initdb-pg_hba.conf"
  cp -p "$STARRING_PGDATA/pg_ident.conf" \
    "$STARRING_CUTOVER_EVIDENCE/initdb-pg_ident.conf"
  install -m 600 ops/postgres/staging-integrated-bootstrap-pg_hba.conf \
    "$STARRING_PGDATA/pg_hba.conf"
  install -m 600 ops/postgres/staging-integrated-bootstrap-pg_ident.conf \
    "$STARRING_PGDATA/pg_ident.conf"
  cmp -s ops/postgres/staging-integrated-bootstrap-pg_hba.conf \
    "$STARRING_PGDATA/pg_hba.conf"
  cmp -s ops/postgres/staging-integrated-bootstrap-pg_ident.conf \
    "$STARRING_PGDATA/pg_ident.conf"
  shasum -a 256 "$STARRING_PGDATA/pg_hba.conf" \
    >"$STARRING_CUTOVER_EVIDENCE/bootstrap-pg_hba.sha256"
  shasum -a 256 "$STARRING_PGDATA/pg_ident.conf" \
    >"$STARRING_CUTOVER_EVIDENCE/bootstrap-pg_ident.sha256"
  shasum -a 256 "$STARRING_BOOTSTRAP_BINARY" \
    >"$STARRING_CUTOVER_EVIDENCE/starring-db-bootstrap.sha256"
  shasum -a 256 "$STARRING_PROVISIONER_BINARY" \
    >"$STARRING_CUTOVER_EVIDENCE/starring-staging-provisioner.sha256"
)
```

The bootstrap files contain no secret, but they authorize a local privilege
transition. Do not leave PostgreSQL running under them after Gate 10A.

## Gate 6: start PostgreSQL and prove the bootstrap boundary

```zsh
(
  set -euo pipefail
  set +x
  brew services start postgresql@16
  READY=0
  for ATTEMPT in {1..60}
  do
    if /opt/homebrew/opt/postgresql@16/bin/pg_isready \
      --host "$STARRING_BOOTSTRAP_SOCKET_DIR" \
      --port 5432 \
      --username "$STARRING_STAGING_CLUSTER_ADMIN" \
      --dbname postgres >/dev/null 2>&1
    then
      READY=1
      break
    fi
    sleep 1
  done
  test "$READY" = 1
  lsof -nP -iTCP:5432 -sTCP:LISTEN \
    >"$STARRING_CUTOVER_EVIDENCE/postgresql-listeners.txt"
  grep -F '127.0.0.1:5432' \
    "$STARRING_CUTOVER_EVIDENCE/postgresql-listeners.txt" >/dev/null
  ! grep -E 'TCP (\\*|0\\.0\\.0\\.0|\\[.*\\]):5432' \
    "$STARRING_CUTOVER_EVIDENCE/postgresql-listeners.txt" >/dev/null
)
```

The following proof uses only the reviewed peer map and never prompts for or
sets a password:

```zsh
(
  set -euo pipefail
  set +x
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  EXPECTED_TARGET="postgres|${STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER}|${STARRING_STAGING_CLUSTER_ADMIN}|${STARRING_STAGING_CLUSTER_ADMIN}|true|16|scram-sha-256|on"
  OBSERVED_TARGET="$(
    /opt/homebrew/opt/postgresql@16/bin/psql \
      --no-psqlrc --set ON_ERROR_STOP=1 --no-password --quiet \
      --host "$STARRING_BOOTSTRAP_SOCKET_DIR" --port 5432 \
      --username "$STARRING_STAGING_CLUSTER_ADMIN" \
      --dbname postgres --tuples-only --no-align \
      --command "BEGIN READ ONLY" \
      --command "SELECT pg_catalog.concat_ws('|', pg_catalog.current_database(), control.system_identifier::TEXT, current_user, session_user, administrator.rolsuper::TEXT, (pg_catalog.current_setting('server_version_num')::INTEGER / 10000)::TEXT, pg_catalog.current_setting('password_encryption'), pg_catalog.current_setting('data_checksums')) FROM pg_catalog.pg_control_system() AS control CROSS JOIN pg_catalog.pg_roles AS administrator WHERE administrator.rolname = current_user" \
      --command "COMMIT"
  )"
  test "$OBSERVED_TARGET" = "$EXPECTED_TARGET"
)
```

Prove all seven bootstrap HBA rules and the one exact ident mapping before
creating the application database:

```zsh
(
  set -euo pipefail
  set +x
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  HBA_PROOF_QUERY="
    WITH expected AS (
      SELECT ARRAY['starring_cluster_admin']::TEXT[] AS administrator
    )
    SELECT pg_catalog.concat_ws(
      '|',
      pg_catalog.count(*) FILTER (WHERE rule.error IS NOT NULL),
      pg_catalog.count(*),
      pg_catalog.count(*) FILTER (
        WHERE rule_number = 1
          AND type = 'local'
          AND database = ARRAY[
            'postgres',
            'starring_runtime_staging'
          ]::TEXT[]
          AND user_name = expected.administrator
          AND address IS NULL
          AND netmask IS NULL
          AND auth_method = 'peer'
          AND options = ARRAY['map=starring_bootstrap']::TEXT[]
      ),
      pg_catalog.count(*) FILTER (
        WHERE rule_number = 2
          AND type = 'host'
          AND database = ARRAY['all']::TEXT[]
          AND user_name = ARRAY['all']::TEXT[]
          AND address = '0.0.0.0'
          AND netmask = '0.0.0.0'
          AND auth_method = 'reject'
          AND options IS NULL
      ),
      pg_catalog.count(*) FILTER (
        WHERE rule_number = 3
          AND type = 'host'
          AND database = ARRAY['all']::TEXT[]
          AND user_name = ARRAY['all']::TEXT[]
          AND address = '::'
          AND netmask = '::'
          AND auth_method = 'reject'
          AND options IS NULL
      ),
      pg_catalog.count(*) FILTER (
        WHERE rule_number = 4
          AND type = 'local'
          AND database = ARRAY['all']::TEXT[]
          AND user_name = ARRAY['all']::TEXT[]
          AND address IS NULL
          AND netmask IS NULL
          AND auth_method = 'reject'
          AND options IS NULL
      ),
      pg_catalog.count(*) FILTER (
        WHERE rule_number = 5
          AND type = 'host'
          AND database = ARRAY['replication']::TEXT[]
          AND user_name = ARRAY['all']::TEXT[]
          AND address = '0.0.0.0'
          AND netmask = '0.0.0.0'
          AND auth_method = 'reject'
          AND options IS NULL
      ),
      pg_catalog.count(*) FILTER (
        WHERE rule_number = 6
          AND type = 'host'
          AND database = ARRAY['replication']::TEXT[]
          AND user_name = ARRAY['all']::TEXT[]
          AND address = '::'
          AND netmask = '::'
          AND auth_method = 'reject'
          AND options IS NULL
      ),
      pg_catalog.count(*) FILTER (
        WHERE rule_number = 7
          AND type = 'local'
          AND database = ARRAY['replication']::TEXT[]
          AND user_name = ARRAY['all']::TEXT[]
          AND address IS NULL
          AND netmask IS NULL
          AND auth_method = 'reject'
          AND options IS NULL
      )
    )
    FROM pg_catalog.pg_hba_file_rules AS rule
    CROSS JOIN expected
  "
  HBA_VALIDATION="$(
    /opt/homebrew/opt/postgresql@16/bin/psql \
      --no-psqlrc --set ON_ERROR_STOP=1 --no-password \
      --host "$STARRING_BOOTSTRAP_SOCKET_DIR" --port 5432 \
      --username "$STARRING_STAGING_CLUSTER_ADMIN" \
      --dbname postgres --tuples-only --no-align \
      --command "$HBA_PROOF_QUERY" \
      --command "SELECT pg_catalog.concat_ws('|', pg_catalog.count(*) FILTER (WHERE mapping.error IS NOT NULL), pg_catalog.count(*), pg_catalog.count(*) FILTER (WHERE map_name = 'starring_bootstrap' AND sys_name = 'jungbogeon' AND pg_username = 'starring_cluster_admin')) FROM pg_catalog.pg_ident_file_mappings AS mapping" \
      | sed '/^$/d'
  )"
  test "$(print -r -- "$HBA_VALIDATION" | sed -n '1p')" \
    = '0|7|1|1|1|1|1|1|1'
  test "$(print -r -- "$HBA_VALIDATION" | sed -n '2p')" \
    = '0|1|1'
  cmp -s "$STARRING_PGDATA/pg_hba.conf" \
    ops/postgres/staging-integrated-bootstrap-pg_hba.conf
  cmp -s "$STARRING_PGDATA/pg_ident.conf" \
    ops/postgres/staging-integrated-bootstrap-pg_ident.conf
)
```

Any mismatch stops the cutover. Do not weaken the expected record to match an
unexpected parser result.

## Gate 7: run the embedded owner and migration bootstrap

The immutable bootstrap binary verifies the exact administrator, PostgreSQL
major, independently reviewed system identifier, acknowledgement, peer rule,
and ident map before mutation. It creates or normalizes the non-login owner and
fixed database, runs the compile-time SQLx migration set under
`SET ROLE starring_owner`, verifies its own exact ledger, verifies every
user-schema relation and all 124 capability functions, resets role, and removes
all inbound and outbound owner memberships. In peer mode it is the sole owner
of cluster-administrator normalization. It creates no migrator login.

```zsh
(
  set -euo pipefail
  set +x
  test -x "$STARRING_BOOTSTRAP_BINARY"
  env -i \
    PATH="/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin" \
    "$STARRING_BOOTSTRAP_BINARY" \
    --peer-bootstrap \
    "$STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER" \
    "$STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT" \
    >"$STARRING_CUTOVER_EVIDENCE/starring-db-bootstrap.txt"
  grep -E \
    '^database=starring_runtime_staging owner=starring_owner migrations=[1-9][0-9]* relations=178 capability_functions=124$' \
    "$STARRING_CUTOVER_EVIDENCE/starring-db-bootstrap.txt" >/dev/null
)
```

Prove the administrator normalization performed by that exact binary. The
administrator remains passwordless until Gate 10:

```zsh
(
  set -euo pipefail
  set +x
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  ADMIN_CONTRACT="$(
    /opt/homebrew/opt/postgresql@16/bin/psql \
      --no-psqlrc --set ON_ERROR_STOP=1 --no-password \
      --host "$STARRING_BOOTSTRAP_SOCKET_DIR" --port 5432 \
      --username "$STARRING_STAGING_CLUSTER_ADMIN" \
      --dbname postgres --tuples-only --no-align \
      --command "
        SELECT pg_catalog.concat_ws(
          '|',
          role.rolsuper::INTEGER,
          role.rolcanlogin::INTEGER,
          (NOT role.rolcreatedb)::INTEGER,
          (NOT role.rolcreaterole)::INTEGER,
          (NOT role.rolinherit)::INTEGER,
          (NOT role.rolreplication)::INTEGER,
          (NOT role.rolbypassrls)::INTEGER,
          role.rolconnlimit,
          (role.rolvaliduntil = 'infinity'::TIMESTAMP WITH TIME ZONE)::INTEGER,
          (role.rolpassword IS NULL)::INTEGER,
          (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_db_role_setting AS setting
            WHERE setting.setrole = role.oid
          ),
          (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_auth_members AS membership
            WHERE membership.roleid = role.oid
               OR membership.member = role.oid
          )
        )
        FROM pg_catalog.pg_authid AS role
        WHERE role.rolname = 'starring_cluster_admin'
      "
  )"
  test "$ADMIN_CONTRACT" = '1|1|1|1|1|1|1|2|1|1|0|0'
)
```

## Gate 8: independently prove the embedded ledger and ownership

Do not retry an interrupted migration blindly. Record its version, SQLSTATE,
and stable redacted error, leave every service and tunnel stopped, and inspect
the ledger before deciding whether a retry is valid.

Compare the exact repository migrations and SHA-384 checksums with the SQLx
ledger:

```zsh
(
  set -euo pipefail
  set +x
  cd /Users/jungbogeon/starring
  EXPECTED_LEDGER="$STARRING_CUTOVER_EVIDENCE/expected-migration-ledger.txt"
  APPLIED_LEDGER="$STARRING_CUTOVER_EVIDENCE/applied-migration-ledger.txt"
  test ! -e "$EXPECTED_LEDGER"
  test ! -e "$APPLIED_LEDGER"
  for MIGRATION in migrations/*.sql
  do
    BASENAME="$(basename "$MIGRATION")"
    print -r -- "$BASENAME" | grep -Eq '^[0-9]+_.+\.sql$'
    VERSION="${BASENAME%%_*}"
    CHECKSUM="$(shasum -a 384 "$MIGRATION" | awk '{ print $1 }')"
    print -r -- "${VERSION}:${CHECKSUM}"
  done | LC_ALL=C sort -t: -k1,1n >"$EXPECTED_LEDGER"
  test "$(
    cut -d: -f1 "$EXPECTED_LEDGER" | uniq -d | wc -l | tr -d ' '
  )" = 0
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  /opt/homebrew/opt/postgresql@16/bin/psql \
    --no-psqlrc --set ON_ERROR_STOP=1 --no-password \
    --host "$STARRING_BOOTSTRAP_SOCKET_DIR" --port 5432 \
    --username "$STARRING_STAGING_CLUSTER_ADMIN" \
    --dbname starring_runtime_staging --tuples-only --no-align \
    --command "SELECT version::TEXT || ':' || CASE WHEN success THEN pg_catalog.encode(checksum, 'hex') ELSE 'failed' END FROM public._sqlx_migrations ORDER BY version" \
    >"$APPLIED_LEDGER"
  diff -u "$EXPECTED_LEDGER" "$APPLIED_LEDGER"
)
```

Require exact ownership after the embedded bootstrap:

```zsh
(
  set -euo pipefail
  set +x
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  OWNERSHIP_PROOF="$(
    /opt/homebrew/opt/postgresql@16/bin/psql \
      --no-psqlrc --set ON_ERROR_STOP=1 --no-password \
      --host "$STARRING_BOOTSTRAP_SOCKET_DIR" --port 5432 \
      --username "$STARRING_STAGING_CLUSTER_ADMIN" \
      --dbname starring_runtime_staging --tuples-only --no-align \
      --command "
        SELECT pg_catalog.concat_ws(
          '|',
          (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_class AS relation
            JOIN pg_catalog.pg_namespace AS namespace
              ON namespace.oid = relation.relnamespace
            WHERE namespace.nspname <> 'information_schema'
              AND pg_catalog.left(namespace.nspname, 3) <> 'pg_'
          ),
          (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_class AS relation
            JOIN pg_catalog.pg_namespace AS namespace
              ON namespace.oid = relation.relnamespace
            WHERE namespace.nspname <> 'information_schema'
              AND pg_catalog.left(namespace.nspname, 3) <> 'pg_'
              AND relation.relowner <> 'starring_owner'::REGROLE
          ),
          (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_proc AS routine
            JOIN pg_catalog.pg_namespace AS namespace
              ON namespace.oid = routine.pronamespace
            WHERE namespace.nspname = 'public'
              AND routine.proowner <> 'starring_owner'::REGROLE
          ),
          (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_type AS type_row
            JOIN pg_catalog.pg_namespace AS namespace
              ON namespace.oid = type_row.typnamespace
            WHERE namespace.nspname = 'public'
              AND type_row.typowner <> 'starring_owner'::REGROLE
          ),
          (
            SELECT (namespace.nspowner = 'starring_owner'::REGROLE)::INTEGER
            FROM pg_catalog.pg_namespace AS namespace
            WHERE namespace.nspname = 'public'
          ),
          (
            SELECT (database_row.datdba = 'starring_owner'::REGROLE)::INTEGER
            FROM pg_catalog.pg_database AS database_row
            WHERE database_row.datname = 'starring_runtime_staging'
          ),
          (
            SELECT (relation.relowner = 'starring_owner'::REGROLE)::INTEGER
            FROM pg_catalog.pg_class AS relation
            JOIN pg_catalog.pg_namespace AS namespace
              ON namespace.oid = relation.relnamespace
            WHERE namespace.nspname = 'public'
              AND relation.relname = '_sqlx_migrations'
          )
        )
      "
  )"
  test "$OWNERSHIP_PROOF" = '171|0|0|0|1|1|1'
)
```

Prove the owner has no membership or role settings and that the obsolete
`starring_migrator` role was never created:

```zsh
(
  set -euo pipefail
  set +x
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  OWNER_POSTFLIGHT="$(
    /opt/homebrew/opt/postgresql@16/bin/psql \
      --no-psqlrc --set ON_ERROR_STOP=1 --no-password \
      --host "$STARRING_BOOTSTRAP_SOCKET_DIR" --port 5432 \
      --username "$STARRING_STAGING_CLUSTER_ADMIN" \
      --dbname starring_runtime_staging --tuples-only --no-align \
      --command "
        SELECT pg_catalog.concat_ws(
          '|',
          (NOT owner.rolcanlogin)::INTEGER,
          (owner.rolpassword IS NULL)::INTEGER,
          (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_auth_members AS membership
            WHERE membership.member = owner.oid
               OR membership.roleid = owner.oid
          ),
          (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_db_role_setting AS setting
            WHERE setting.setrole = owner.oid
          ),
          (pg_catalog.to_regrole('starring_migrator') IS NULL)::INTEGER
        )
        FROM pg_catalog.pg_authid AS owner
        WHERE owner.rolname = 'starring_owner'
      "
  )"
  test "$OWNER_POSTFLIGHT" = '1|1|0|0|1'
)
```

## Gate 9: quarantine both application role sets

Both staging services and the Starring tunnel must still be unloaded. Run the
API quarantine first, then the runtime quarantine. The runtime quarantine is
last because it applies the v2 cluster-wide `PUBLIC` and bidirectional
membership contract.

```zsh
(
  set -euo pipefail
  set +x
  cd /Users/jungbogeon/starring
  DOMAIN="gui/$(id -u)"
  ! launchctl print "$DOMAIN/local.starring.api.staging" >/dev/null 2>&1
  ! launchctl print "$DOMAIN/local.starring.runtime.staging" >/dev/null 2>&1
  ! launchctl print "$DOMAIN/$STARRING_STAGING_TUNNEL_LABEL" >/dev/null 2>&1
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  PGOPTIONS="-c starring.expected_staging_database=starring_runtime_staging -c starring.expected_staging_system_identifier=${STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER}" \
    /opt/homebrew/opt/postgresql@16/bin/psql \
      --no-psqlrc --set ON_ERROR_STOP=1 --no-password \
      --host "$STARRING_BOOTSTRAP_SOCKET_DIR" --port 5432 \
      --username "$STARRING_STAGING_CLUSTER_ADMIN" \
      --dbname starring_runtime_staging \
      --file ops/postgres/staging-api-role-bootstrap.sql
  unset PGOPTIONS
)
```

```zsh
(
  set -euo pipefail
  set +x
  cd /Users/jungbogeon/starring
  DOMAIN="gui/$(id -u)"
  ! launchctl print "$DOMAIN/local.starring.api.staging" >/dev/null 2>&1
  ! launchctl print "$DOMAIN/local.starring.runtime.staging" >/dev/null 2>&1
  ! launchctl print "$DOMAIN/$STARRING_STAGING_TUNNEL_LABEL" >/dev/null 2>&1
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  /opt/homebrew/opt/postgresql@16/bin/psql \
    --no-psqlrc --set ON_ERROR_STOP=1 --no-password \
    --host "$STARRING_BOOTSTRAP_SOCKET_DIR" --port 5432 \
    --username "$STARRING_STAGING_CLUSTER_ADMIN" \
    --set runtime_enable=off \
    --set expected_database=starring_runtime_staging \
    --set expected_system_identifier="$STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER" \
    --set runtime_dedicated_cluster_acknowledgement="$STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT" \
    --dbname starring_runtime_staging \
    --command "SELECT 1 / CASE WHEN NOT EXISTS (SELECT 1 FROM pg_catalog.pg_stat_activity WHERE pid <> pg_catalog.pg_backend_pid() AND (backend_type = 'client backend' OR usesysid IN (pg_catalog.to_regrole('starring_runtime_execution'), pg_catalog.to_regrole('starring_runtime_exact_target'), pg_catalog.to_regrole('starring_runtime_panel'), pg_catalog.to_regrole('starring_runtime_serving'), pg_catalog.to_regrole('starring_runtime_interaction')))) AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_prepared_xacts) THEN 1 ELSE 0 END AS cluster_quiescence_proof" \
    --file ops/postgres/staging-runtime-role-bootstrap.sql
)
```

A failure in either manifest leaves staging offline. Rerun both quarantine
blocks in the same order after correcting the reported drift. Never continue
from a partial role manifest.

## Gate 10: provision twenty-one credentials and three keyrings

Keep all services stopped. The immutable one-shot provisioner checks the peer
boundary, independently approved system identifier and acknowledgement,
database quiescence, passwordless owner, and all twenty passwordless
`NOLOGIN` application roles. It also requires the three pre-existing Discord
provider items to be readable and verifies that the API and runtime bot-token
items are identical without printing either value. The separately preflighted
authoring-worker item is not part of provisioner mutation or semantic reads.

It generates twenty-one distinct 32-byte random passwords, two independent API
keyrings, one dedicated runtime interaction-token envelope keyring, and
independent PostgreSQL SCRAM-SHA-256 verifiers with 4,096
PBKDF2-HMAC-SHA-256 iterations. It writes twenty fixed application URLs, one
fixed administrator URL, and three keyring objects to Keychain, then applies
only verifier strings in one database transaction. All twenty-four managed
Keychain writes share one rollback boundary. It restores every prior managed
Keychain value, including removal of a newly created runtime keyring item, on
a pre-commit failure. A
`database_commit_indeterminate` result is an incident: preserve the new
values, keep all clients stopped, and reconcile database and Keychain state
without rerunning.

```zsh
(
  set -euo pipefail
  set +x
  test -x "$STARRING_PROVISIONER_BINARY"
  env -i \
    PATH="/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin" \
    "$STARRING_PROVISIONER_BINARY" \
    "$STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER" \
    "$STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT" \
    >"$STARRING_CUTOVER_EVIDENCE/starring-staging-provisioner.txt"
  grep -E \
    '^provisioned database=starring_runtime_staging application_database_credentials=20 keyrings=3 product_action_key_id=[A-Za-z0-9_-]+ snapshot_envelope_key_id=[A-Za-z0-9_-]+ interaction_token_envelope_key_id=[A-Za-z0-9_-]+$' \
    "$STARRING_CUTOVER_EVIDENCE/starring-staging-provisioner.txt" >/dev/null
)
```

The twenty application roles remain `NOLOGIN`. Verify only aggregate state
and verifier format, never verifier text:

```zsh
(
  set -euo pipefail
  set +x
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  PASSWORD_PROOF="$(
    /opt/homebrew/opt/postgresql@16/bin/psql \
      --no-psqlrc --set ON_ERROR_STOP=1 --no-password \
      --host "$STARRING_BOOTSTRAP_SOCKET_DIR" --port 5432 \
      --username "$STARRING_STAGING_CLUSTER_ADMIN" \
      --dbname starring_runtime_staging --tuples-only --no-align \
      --command "
        WITH managed(role_name) AS (
          VALUES
            ('starring_identity_oauth'),
            ('starring_identity_issuer'),
            ('starring_identity_session'),
            ('starring_identity_security'),
            ('starring_installation_authority_reader'),
            ('starring_authorized_snapshot_reader'),
            ('starring_promotion_executor'),
            ('starring_decision_reader'),
            ('starring_decision_approval'),
            ('starring_decision_rejection'),
            ('starring_decision_apply'),
            ('starring_decision_cancellation'),
            ('starring_deployment_status_reader'),
            ('starring_operational_deployment_status_reader'),
            ('starring_authoring_session_writer'),
            ('starring_runtime_execution'),
            ('starring_runtime_exact_target'),
            ('starring_runtime_panel'),
            ('starring_runtime_serving'),
            ('starring_runtime_interaction')
        )
        SELECT pg_catalog.concat_ws(
          '|',
          pg_catalog.count(*),
          pg_catalog.count(*) FILTER (
            WHERE role.rolpassword LIKE 'SCRAM-SHA-256$%'
          ),
          pg_catalog.count(*) FILTER (WHERE NOT role.rolcanlogin)
        )
        FROM managed
        JOIN pg_catalog.pg_authid AS role
          ON role.rolname = managed.role_name
      "
  )"
  test "$PASSWORD_PROOF" = '20|20|20'
  ADMIN_PASSWORD_PROOF="$(
    /opt/homebrew/opt/postgresql@16/bin/psql \
      --no-psqlrc --set ON_ERROR_STOP=1 --no-password \
      --host "$STARRING_BOOTSTRAP_SOCKET_DIR" --port 5432 \
      --username "$STARRING_STAGING_CLUSTER_ADMIN" \
      --dbname postgres --tuples-only --no-align \
      --command "
        SELECT pg_catalog.concat_ws(
          '|',
          pg_catalog.count(*),
          pg_catalog.count(*) FILTER (
            WHERE role.rolpassword LIKE 'SCRAM-SHA-256$%'
          ),
          pg_catalog.count(*) FILTER (
            WHERE role.rolsuper
              AND role.rolcanlogin
              AND NOT role.rolcreatedb
              AND NOT role.rolcreaterole
              AND NOT role.rolinherit
              AND NOT role.rolreplication
              AND NOT role.rolbypassrls
              AND role.rolconnlimit = 2
              AND role.rolvaliduntil = 'infinity'::TIMESTAMP WITH TIME ZONE
          ),
          (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_db_role_setting AS setting
            WHERE setting.setrole = pg_catalog.to_regrole(
              'starring_cluster_admin'
            )
          ),
          (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_auth_members AS membership
            WHERE membership.roleid = pg_catalog.to_regrole(
                    'starring_cluster_admin'
                  )
               OR membership.member = pg_catalog.to_regrole(
                    'starring_cluster_admin'
                  )
          )
        )
        FROM pg_catalog.pg_authid AS role
        WHERE role.rolname = 'starring_cluster_admin'
      "
  )"
  test "$ADMIN_PASSWORD_PROOF" = '1|1|1|0|0'
)
```

Enable the runtime roles first and the API roles second while the bootstrap
HBA still rejects every application transport. Both services and the tunnel
remain unloaded. This ordering lets the runtime manifest re-prove the v2
cluster-wide boundary before any final application path exists:

```zsh
(
  set -euo pipefail
  set +x
  cd /Users/jungbogeon/starring
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  /opt/homebrew/opt/postgresql@16/bin/psql \
    --no-psqlrc --set ON_ERROR_STOP=1 --no-password \
    --host "$STARRING_BOOTSTRAP_SOCKET_DIR" --port 5432 \
    --username "$STARRING_STAGING_CLUSTER_ADMIN" \
    --set runtime_enable=on \
    --set expected_database=starring_runtime_staging \
    --set expected_system_identifier="$STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER" \
    --set runtime_dedicated_cluster_acknowledgement="$STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT" \
    --dbname starring_runtime_staging \
    --file ops/postgres/staging-runtime-role-bootstrap.sql
)
```

```zsh
(
  set -euo pipefail
  set +x
  cd /Users/jungbogeon/starring
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  PGOPTIONS="-c starring.expected_staging_database=starring_runtime_staging -c starring.expected_staging_system_identifier=${STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER}" \
    /opt/homebrew/opt/postgresql@16/bin/psql \
      --no-psqlrc --set ON_ERROR_STOP=1 --no-password \
      --host "$STARRING_BOOTSTRAP_SOCKET_DIR" --port 5432 \
      --username "$STARRING_STAGING_CLUSTER_ADMIN" \
      --dbname starring_runtime_staging \
      --file ops/postgres/staging-api-role-enable.sql
  unset PGOPTIONS
)
```

If provisioning or either enable manifest fails before the first final-HBA
`mv`, run the Gate 9 peer quarantine in API-first, runtime-last order and stop.
Only a reviewed reconciliation may begin a new Gate 10 attempt.

## Gate 10A: atomically replace peer bootstrap with the final HBA

This is the security-boundary transition. The final manifest exposes only the
twenty exact roles on IPv4 loopback to the fixed database and the cluster
administrator on IPv4 loopback to `postgres` and the fixed database. It has no
migrator path, no peer path, no IPv6 allow, no socket allow, and no physical
replication allow.

First archive the active bootstrap files, remove any auto-configuration
override, and stage a main configuration that disables Unix sockets for the
next postmaster start while the peer path is still active. Then stage both
replacement access files in PGDATA, stop and prove PostgreSQL down, atomically
rename the final HBA into place, remove the now-unused ident mapping, install
the staged main configuration, and restart:

```zsh
(
  set -euo pipefail
  set +x
  cd /Users/jungbogeon/starring
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  DOMAIN="gui/$(id -u)"
  ! launchctl print "$DOMAIN/local.starring.api.staging" >/dev/null 2>&1
  ! launchctl print "$DOMAIN/local.starring.runtime.staging" >/dev/null 2>&1
  ! launchctl print "$DOMAIN/$STARRING_STAGING_TUNNEL_LABEL" >/dev/null 2>&1
  cmp -s "$STARRING_PGDATA/pg_hba.conf" \
    ops/postgres/staging-integrated-bootstrap-pg_hba.conf
  cmp -s "$STARRING_PGDATA/pg_ident.conf" \
    ops/postgres/staging-integrated-bootstrap-pg_ident.conf
  test "$(wc -l <ops/postgres/staging-integrated-pg_hba.conf | tr -d ' ')" = 15
  cp -p "$STARRING_PGDATA/pg_hba.conf" \
    "$STARRING_CUTOVER_EVIDENCE/active-bootstrap-pg_hba.conf"
  cp -p "$STARRING_PGDATA/pg_ident.conf" \
    "$STARRING_CUTOVER_EVIDENCE/active-bootstrap-pg_ident.conf"
  cp -p "$STARRING_PGDATA/postgresql.conf" \
    "$STARRING_CUTOVER_EVIDENCE/active-bootstrap-postgresql.conf"
  cp -p "$STARRING_PGDATA/postgresql.auto.conf" \
    "$STARRING_CUTOVER_EVIDENCE/active-bootstrap-postgresql.auto.conf"
  /opt/homebrew/opt/postgresql@16/bin/psql \
    --no-psqlrc --set ON_ERROR_STOP=1 --no-password \
    --host "$STARRING_BOOTSTRAP_SOCKET_DIR" --port 5432 \
    --username "$STARRING_STAGING_CLUSTER_ADMIN" \
    --dbname postgres \
    --command "ALTER SYSTEM RESET unix_socket_directories"
  ! grep -Eq \
    '^[[:space:]]*unix_socket_directories[[:space:]]*=' \
    "$STARRING_PGDATA/postgresql.auto.conf"
  cp -p "$STARRING_PGDATA/postgresql.auto.conf" \
    "$STARRING_CUTOVER_EVIDENCE/post-reset-postgresql.auto.conf"
  FINAL_HBA_TEMP="$STARRING_PGDATA/.pg_hba.conf.final-${STARRING_CUTOVER_ID}"
  FINAL_IDENT_TEMP="$STARRING_PGDATA/.pg_ident.conf.final-${STARRING_CUTOVER_ID}"
  FINAL_CONFIG_TEMP="$STARRING_PGDATA/.postgresql.conf.final-${STARRING_CUTOVER_ID}"
  test ! -e "$FINAL_HBA_TEMP"
  test ! -e "$FINAL_IDENT_TEMP"
  test ! -e "$FINAL_CONFIG_TEMP"
  install -m 600 ops/postgres/staging-integrated-pg_hba.conf \
    "$FINAL_HBA_TEMP"
  install -m 600 /dev/null "$FINAL_IDENT_TEMP"
  cp -p "$STARRING_PGDATA/postgresql.conf" "$FINAL_CONFIG_TEMP"
  test "$(
    grep -Ec \
      "^[[:space:]]*unix_socket_directories[[:space:]]*=[[:space:]]*'/private/tmp/starring-bootstrap'([[:space:]]*#.*)?$" \
      "$FINAL_CONFIG_TEMP"
  )" = 1
  /usr/bin/sed -E -i '' \
    "s|^[[:space:]]*unix_socket_directories[[:space:]]*=[[:space:]]*'/private/tmp/starring-bootstrap'([[:space:]]*#.*)?\$|unix_socket_directories = '' # comma-separated list of directories|" \
    "$FINAL_CONFIG_TEMP"
  cmp -s ops/postgres/staging-integrated-pg_hba.conf "$FINAL_HBA_TEMP"
  test ! -s "$FINAL_IDENT_TEMP"
  test "$(
    grep -Fxc \
      "unix_socket_directories = '' # comma-separated list of directories" \
      "$FINAL_CONFIG_TEMP"
  )" = 1
  ! grep -F '/private/tmp/starring-bootstrap' "$FINAL_CONFIG_TEMP" >/dev/null
  STAGED_SOCKET_DIRECTORIES="$(
    /opt/homebrew/opt/postgresql@16/bin/postgres \
      -D "$STARRING_PGDATA" \
      -C unix_socket_directories \
      -c "config_file=$FINAL_CONFIG_TEMP"
  )"
  test -z "$STAGED_SOCKET_DIRECTORIES"
  brew services stop postgresql@16
  for ATTEMPT in {1..60}
  do
    if ! lsof -nP -iTCP:5432 -sTCP:LISTEN >/dev/null 2>&1
    then
      break
    fi
    sleep 1
  done
  ! lsof -nP -iTCP:5432 -sTCP:LISTEN >/dev/null 2>&1
  ! /opt/homebrew/opt/postgresql@16/bin/pg_ctl \
    --pgdata "$STARRING_PGDATA" status >/dev/null 2>&1
  test ! -e "$STARRING_PGDATA/postmaster.pid"
  test ! -e "$STARRING_BOOTSTRAP_SOCKET_DIR/.s.PGSQL.5432"
  test ! -e "$STARRING_BOOTSTRAP_SOCKET_DIR/.s.PGSQL.5432.lock"
  mv "$FINAL_HBA_TEMP" "$STARRING_PGDATA/pg_hba.conf"
  mv "$FINAL_IDENT_TEMP" "$STARRING_PGDATA/pg_ident.conf"
  mv "$FINAL_CONFIG_TEMP" "$STARRING_PGDATA/postgresql.conf"
  sync
  brew services start postgresql@16
  READY=0
  for ATTEMPT in {1..60}
  do
    if /opt/homebrew/opt/postgresql@16/bin/pg_isready \
      --host 127.0.0.1 --port 5432 >/dev/null 2>&1
    then
      READY=1
      break
    fi
    sleep 1
  done
  test "$READY" = 1
  lsof -nP -iTCP:5432 -sTCP:LISTEN \
    >"$STARRING_CUTOVER_EVIDENCE/final-postgresql-listeners.txt"
  grep -F '127.0.0.1:5432' \
    "$STARRING_CUTOVER_EVIDENCE/final-postgresql-listeners.txt" >/dev/null
  ! grep -E 'TCP (\\*|0\\.0\\.0\\.0|\\[.*\\]):5432' \
    "$STARRING_CUTOVER_EVIDENCE/final-postgresql-listeners.txt" >/dev/null
  test ! -e "$STARRING_BOOTSTRAP_SOCKET_DIR/.s.PGSQL.5432"
  test ! -e "$STARRING_BOOTSTRAP_SOCKET_DIR/.s.PGSQL.5432.lock"
  FINAL_CONFIG_SOCKET_DIRECTORIES="$(
    /opt/homebrew/opt/postgresql@16/bin/postgres \
      -D "$STARRING_PGDATA" \
      -C unix_socket_directories
  )"
  test -z "$FINAL_CONFIG_SOCKET_DIRECTORIES"
  LIVE_SOCKET_DIRECTORIES="$(
    starring_with_admin_pgpass /usr/bin/env \
      PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
      --no-psqlrc --set ON_ERROR_STOP=1 --no-password \
      --host 127.0.0.1 --port 5432 \
      --username "$STARRING_STAGING_CLUSTER_ADMIN" \
      --dbname postgres --tuples-only --no-align \
      --command "SELECT pg_catalog.current_setting('unix_socket_directories')"
  )"
  test -z "$LIVE_SOCKET_DIRECTORIES"
  cmp -s "$STARRING_PGDATA/pg_hba.conf" \
    ops/postgres/staging-integrated-pg_hba.conf
  test ! -s "$STARRING_PGDATA/pg_ident.conf"
  cp -p "$STARRING_PGDATA/pg_hba.conf" \
    "$STARRING_CUTOVER_EVIDENCE/final-pg_hba.conf"
  cp -p "$STARRING_PGDATA/pg_ident.conf" \
    "$STARRING_CUTOVER_EVIDENCE/final-pg_ident.conf"
  cp -p "$STARRING_PGDATA/postgresql.conf" \
    "$STARRING_CUTOVER_EVIDENCE/final-postgresql.conf"
  cp -p "$STARRING_PGDATA/postgresql.auto.conf" \
    "$STARRING_CUTOVER_EVIDENCE/final-postgresql.auto.conf"
  shasum -a 256 \
    "$STARRING_PGDATA/pg_hba.conf" \
    "$STARRING_PGDATA/pg_ident.conf" \
    "$STARRING_PGDATA/postgresql.conf" \
    "$STARRING_PGDATA/postgresql.auto.conf" \
    >"$STARRING_CUTOVER_EVIDENCE/final-postgresql-config.sha256"
)
```

PostgreSQL is proven stopped before the first `mv`, so a rename or sync failure
cannot leave the live peer boundary active with partially replaced files. Any
failure after the first `mv` is fail-closed: keep both applications and the
tunnel unloaded, stop PostgreSQL if a start was attempted, and use the physical
rollback. Never reinstall the peer files merely to continue forward. A reload
is insufficient because removing `unix_socket_directories` is a
postmaster-start setting.

Prove all fifteen parsed final rules in exact order and prove the ident mapping
is empty. Administrator authentication uses only the ephemeral Keychain-backed
`PGPASSFILE`:

```zsh
(
  set -euo pipefail
  set +x
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  FINAL_HBA_PROOF_QUERY="
    WITH expected AS (
      SELECT
        ARRAY[
          'starring_runtime_execution',
          'starring_runtime_exact_target',
          'starring_runtime_panel',
          'starring_runtime_serving',
          'starring_runtime_interaction'
        ]::TEXT[] AS runtime_roles,
        ARRAY[
          'starring_identity_oauth',
          'starring_identity_issuer',
          'starring_identity_session',
          'starring_identity_security',
          'starring_installation_authority_reader',
          'starring_authorized_snapshot_reader',
          'starring_promotion_executor',
          'starring_decision_reader',
          'starring_decision_approval',
          'starring_decision_rejection',
          'starring_decision_apply',
          'starring_decision_cancellation',
          'starring_deployment_status_reader',
          'starring_operational_deployment_status_reader',
          'starring_authoring_session_writer'
        ]::TEXT[] AS api_roles,
        ARRAY['starring_cluster_admin']::TEXT[] AS administrator
    )
    SELECT pg_catalog.concat_ws(
      '|',
      pg_catalog.count(*) FILTER (WHERE rule.error IS NOT NULL),
      pg_catalog.count(*),
      pg_catalog.count(*) FILTER (
        WHERE rule_number = 1
          AND type = 'hostnossl'
          AND database = ARRAY['starring_runtime_staging']::TEXT[]
          AND user_name = expected.runtime_roles
          AND address = '127.0.0.1'
          AND netmask = '255.255.255.255'
          AND auth_method = 'scram-sha-256'
          AND options IS NULL
      ),
      pg_catalog.count(*) FILTER (
        WHERE rule_number = 2
          AND type = 'host'
          AND database = ARRAY['all']::TEXT[]
          AND user_name = expected.runtime_roles
          AND address = '0.0.0.0'
          AND netmask = '0.0.0.0'
          AND auth_method = 'reject'
          AND options IS NULL
      ),
      pg_catalog.count(*) FILTER (
        WHERE rule_number = 3
          AND type = 'host'
          AND database = ARRAY['all']::TEXT[]
          AND user_name = expected.runtime_roles
          AND address = '::'
          AND netmask = '::'
          AND auth_method = 'reject'
          AND options IS NULL
      ),
      pg_catalog.count(*) FILTER (
        WHERE rule_number = 4
          AND type = 'local'
          AND database = ARRAY['all']::TEXT[]
          AND user_name = expected.runtime_roles
          AND address IS NULL
          AND netmask IS NULL
          AND auth_method = 'reject'
          AND options IS NULL
      ),
      pg_catalog.count(*) FILTER (
        WHERE rule_number = 5
          AND type = 'hostnossl'
          AND database = ARRAY['starring_runtime_staging']::TEXT[]
          AND user_name = expected.api_roles
          AND address = '127.0.0.1'
          AND netmask = '255.255.255.255'
          AND auth_method = 'scram-sha-256'
          AND options IS NULL
      ),
      pg_catalog.count(*) FILTER (
        WHERE rule_number = 6
          AND type = 'host'
          AND database = ARRAY['all']::TEXT[]
          AND user_name = expected.api_roles
          AND address = '0.0.0.0'
          AND netmask = '0.0.0.0'
          AND auth_method = 'reject'
          AND options IS NULL
      ),
      pg_catalog.count(*) FILTER (
        WHERE rule_number = 7
          AND type = 'host'
          AND database = ARRAY['all']::TEXT[]
          AND user_name = expected.api_roles
          AND address = '::'
          AND netmask = '::'
          AND auth_method = 'reject'
          AND options IS NULL
      ),
      pg_catalog.count(*) FILTER (
        WHERE rule_number = 8
          AND type = 'local'
          AND database = ARRAY['all']::TEXT[]
          AND user_name = expected.api_roles
          AND address IS NULL
          AND netmask IS NULL
          AND auth_method = 'reject'
          AND options IS NULL
      ),
      pg_catalog.count(*) FILTER (
        WHERE rule_number = 9
          AND type = 'hostnossl'
          AND database = ARRAY[
            'postgres',
            'starring_runtime_staging'
          ]::TEXT[]
          AND user_name = expected.administrator
          AND address = '127.0.0.1'
          AND netmask = '255.255.255.255'
          AND auth_method = 'scram-sha-256'
          AND options IS NULL
      ),
      pg_catalog.count(*) FILTER (
        WHERE rule_number = 10
          AND type = 'host'
          AND database = ARRAY['all']::TEXT[]
          AND user_name = ARRAY['all']::TEXT[]
          AND address = '0.0.0.0'
          AND netmask = '0.0.0.0'
          AND auth_method = 'reject'
          AND options IS NULL
      ),
      pg_catalog.count(*) FILTER (
        WHERE rule_number = 11
          AND type = 'host'
          AND database = ARRAY['all']::TEXT[]
          AND user_name = ARRAY['all']::TEXT[]
          AND address = '::'
          AND netmask = '::'
          AND auth_method = 'reject'
          AND options IS NULL
      ),
      pg_catalog.count(*) FILTER (
        WHERE rule_number = 12
          AND type = 'local'
          AND database = ARRAY['all']::TEXT[]
          AND user_name = ARRAY['all']::TEXT[]
          AND address IS NULL
          AND netmask IS NULL
          AND auth_method = 'reject'
          AND options IS NULL
      ),
      pg_catalog.count(*) FILTER (
        WHERE rule_number = 13
          AND type = 'host'
          AND database = ARRAY['replication']::TEXT[]
          AND user_name = ARRAY['all']::TEXT[]
          AND address = '0.0.0.0'
          AND netmask = '0.0.0.0'
          AND auth_method = 'reject'
          AND options IS NULL
      ),
      pg_catalog.count(*) FILTER (
        WHERE rule_number = 14
          AND type = 'host'
          AND database = ARRAY['replication']::TEXT[]
          AND user_name = ARRAY['all']::TEXT[]
          AND address = '::'
          AND netmask = '::'
          AND auth_method = 'reject'
          AND options IS NULL
      ),
      pg_catalog.count(*) FILTER (
        WHERE rule_number = 15
          AND type = 'local'
          AND database = ARRAY['replication']::TEXT[]
          AND user_name = ARRAY['all']::TEXT[]
          AND address IS NULL
          AND netmask IS NULL
          AND auth_method = 'reject'
          AND options IS NULL
      )
    )
    FROM pg_catalog.pg_hba_file_rules AS rule
    CROSS JOIN expected
  "
  FINAL_HBA_PROOF="$(
    starring_with_admin_pgpass /usr/bin/env \
      PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
      --no-psqlrc --set ON_ERROR_STOP=1 --no-password \
      --host 127.0.0.1 --port 5432 \
      --username "$STARRING_STAGING_CLUSTER_ADMIN" \
      --dbname postgres --tuples-only --no-align \
      --command "$FINAL_HBA_PROOF_QUERY"
  )"
  test "$FINAL_HBA_PROOF" \
    = '0|15|1|1|1|1|1|1|1|1|1|1|1|1|1|1|1'
  FINAL_IDENT_PROOF="$(
    starring_with_admin_pgpass /usr/bin/env \
      PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
      --no-psqlrc --set ON_ERROR_STOP=1 --no-password \
      --host 127.0.0.1 --port 5432 \
      --username "$STARRING_STAGING_CLUSTER_ADMIN" \
      --dbname postgres --tuples-only --no-align \
      --command "SELECT pg_catalog.concat_ws('|', pg_catalog.count(*) FILTER (WHERE mapping.error IS NOT NULL), pg_catalog.count(*)) FROM pg_catalog.pg_ident_file_mappings AS mapping"
  )"
  test "$FINAL_IDENT_PROOF" = '0|0'
)
```

Prove the two positive administrator paths, the denied third database path,
both application rule families, peer removal, and the live IPv4 physical
replication rejection. The exact catalog proof covers the dormant IPv6 and
socket replication rules, while the listener and socket checks prove those
transports are unavailable. Administrator authentication flows from Keychain
through an ephemeral `PGPASSFILE`; it never enters an argument or shell
variable:

```zsh
(
  set -euo pipefail
  set +x
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  for DATABASE in postgres starring_runtime_staging
  do
    OBSERVED="$(
      starring_with_admin_pgpass /usr/bin/env \
        PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
        --no-psqlrc --set ON_ERROR_STOP=1 --no-password \
        --host 127.0.0.1 --port 5432 \
        --username "$STARRING_STAGING_CLUSTER_ADMIN" \
        --dbname "$DATABASE" --tuples-only --no-align \
        --command "SELECT pg_catalog.current_database() || '|' || current_user"
    )"
    test "$OBSERVED" \
      = "${DATABASE}|${STARRING_STAGING_CLUSTER_ADMIN}"
  done
  ERROR_PATH="$STARRING_CUTOVER_EVIDENCE/admin-wrong-database.txt"
  if LC_ALL=C starring_with_admin_pgpass /usr/bin/env \
    PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
      --no-psqlrc --set ON_ERROR_STOP=1 --no-password \
      --host 127.0.0.1 --port 5432 \
      --username "$STARRING_STAGING_CLUSTER_ADMIN" \
      --dbname template1 \
      --command "SELECT 1" >/dev/null 2>"$ERROR_PATH"
  then
    exit 1
  fi
  grep -F 'pg_hba.conf rejects connection' "$ERROR_PATH" >/dev/null
  grep -F 'database "template1"' "$ERROR_PATH" >/dev/null
  for ROLE in starring_runtime_execution starring_identity_oauth
  do
    ERROR_PATH="$STARRING_CUTOVER_EVIDENCE/final-hba-target-${ROLE}.txt"
    if LC_ALL=C PGSSLMODE=disable \
      PGPASSWORD='starring-invalid-password-probe' \
      /opt/homebrew/opt/postgresql@16/bin/psql \
        --no-psqlrc --set ON_ERROR_STOP=1 \
        --host 127.0.0.1 --port 5432 \
        --username "$ROLE" \
        --dbname starring_runtime_staging \
        --command "SELECT 1" >/dev/null 2>"$ERROR_PATH"
    then
      exit 1
    fi
    grep -F "password authentication failed for user \"$ROLE\"" \
      "$ERROR_PATH" >/dev/null
    for TARGET in "127.0.0.1:postgres"
    do
      HOST="${TARGET%:*}"
      DATABASE="${TARGET##*:}"
      ERROR_PATH="$STARRING_CUTOVER_EVIDENCE/final-hba-deny-${ROLE}-${DATABASE}-$(print -r -- "$HOST" | tr '/:' '__').txt"
      if LC_ALL=C PGSSLMODE=disable \
        PGPASSWORD='starring-invalid-password-probe' \
        /opt/homebrew/opt/postgresql@16/bin/psql \
          --no-psqlrc --set ON_ERROR_STOP=1 \
          --host "$HOST" --port 5432 \
          --username "$ROLE" \
          --dbname "$DATABASE" \
          --command "SELECT 1" >/dev/null 2>"$ERROR_PATH"
      then
        exit 1
      fi
      grep -F 'pg_hba.conf rejects connection' "$ERROR_PATH" >/dev/null
    done
  done
  unset PGPASSWORD
)
```

```zsh
(
  set -euo pipefail
  set +x
  PEER_ERROR="$STARRING_CUTOVER_EVIDENCE/peer-bootstrap-after-final.txt"
  if env -i \
    PATH="/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin" \
    "$STARRING_BOOTSTRAP_BINARY" \
    --peer-bootstrap \
    "$STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER" \
    "$STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT" \
    >"$PEER_ERROR" 2>&1
  then
    exit 1
  fi
  grep -Fx admin_connection_failed "$PEER_ERROR" >/dev/null
  for HOST in 127.0.0.1
  do
    ERROR_PATH="$STARRING_CUTOVER_EVIDENCE/replication-deny-$(print -r -- "$HOST" | tr '/:' '__').txt"
    if LC_ALL=C \
      PGPASSWORD='starring-invalid-password-probe' \
      /opt/homebrew/opt/postgresql@16/bin/psql \
        --no-psqlrc --set ON_ERROR_STOP=1 \
        --dbname "host=${HOST} port=5432 user=starring_cluster_admin dbname=postgres sslmode=disable replication=true" \
        --command "IDENTIFY_SYSTEM" >/dev/null 2>"$ERROR_PATH"
    then
      exit 1
    fi
    grep -F 'pg_hba.conf rejects replication connection' \
      "$ERROR_PATH" >/dev/null
  done
  unset PGPASSWORD
)
```

The final HBA and these probes supersede every HBA installation or proof block
in the component runbooks.

### Existing nineteen-role staging cluster: incremental authoring writer

This subsection applies only to the already provisioned staging cluster that
has fourteen API roles, five runtime roles, the final TCP administrator
credential, and no authoring-writer role or Keychain item. Do not rerun the
one-shot provisioner and do not replay either full role bootstrap against that
cluster.

Keep the tunnel, API, and runtime stopped while applying the trusted
authoring-writer migration. Before the migration, prove only the new item is
absent without reading any Keychain value. Apply the complete approved
migration set through the immutable bootstrap binary's fixed Keychain mode
while the legacy administrator HBA is still active. The bootstrap verifies the
complete migration ledger, checksums, relation ownership, capability ownership,
database isolation, and exact cluster identity. Independently retain its exact
receipt and the successful ledger cardinality:

```zsh
(
  set -euo pipefail
  set +x
  cd /Users/jungbogeon/starring
  DOMAIN="gui/$(id -u)"
  ! launchctl print \
    "$DOMAIN/$STARRING_STAGING_TUNNEL_LABEL" >/dev/null 2>&1
  ! launchctl print "$DOMAIN/local.starring.api.staging" >/dev/null 2>&1
  ! launchctl print "$DOMAIN/local.starring.runtime.staging" >/dev/null 2>&1
  ! /usr/bin/security find-generic-password \
    -s starring-api.staging \
    -a database.authoring-session-writer >/dev/null 2>&1
  env -i PATH="/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin" \
    "$STARRING_BOOTSTRAP_BINARY" \
    --keychain-admin \
    "$STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER" \
    "$STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT" \
    >"$STARRING_CUTOVER_EVIDENCE/authoring-writer-migration.txt"
  grep -Fx \
    'database=starring_runtime_staging owner=starring_owner migrations=89 relations=171 capability_functions=102' \
    "$STARRING_CUTOVER_EVIDENCE/authoring-writer-migration.txt" >/dev/null
  MIGRATION_LEDGER_PROOF="$(
    starring_with_admin_pgpass /usr/bin/env \
      PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
      --no-psqlrc --set ON_ERROR_STOP=1 --no-password \
      --host 127.0.0.1 --port 5432 \
      --username "$STARRING_STAGING_CLUSTER_ADMIN" \
      --dbname starring_runtime_staging --tuples-only --no-align \
      --command "SELECT pg_catalog.concat_ws('|', pg_catalog.count(*), pg_catalog.count(*) FILTER (WHERE success), pg_catalog.max(version)) FROM public._sqlx_migrations"
  )"
  test "$MIGRATION_LEDGER_PROOF" = '89|89|202607300001'
)
```

Only after that succeeds, archive the active nineteen-role HBA, stage the
reviewed replacement inside PGDATA, compare it byte-for-byte with the approved
manifest, atomically rename it into place, sync, reload, and prove that
PostgreSQL parsed all fifteen rules without an error. The active legacy HBA
must differ from the replacement; this assertion distinguishes historical
pre-incremental evidence from final twenty-role state. Build and install the
bootstrap and provisioner from the same approved revision as the migration and
HBA:

```zsh
(
  set -euo pipefail
  set +x
  cd /Users/jungbogeon/starring
  DOMAIN="gui/$(id -u)"
  ! launchctl print \
    "$DOMAIN/$STARRING_STAGING_TUNNEL_LABEL" >/dev/null 2>&1
  ! launchctl print "$DOMAIN/local.starring.api.staging" >/dev/null 2>&1
  ! launchctl print "$DOMAIN/local.starring.runtime.staging" >/dev/null 2>&1
  ! /usr/bin/security find-generic-password \
    -s starring-api.staging \
    -a database.authoring-session-writer >/dev/null 2>&1
  LEGACY_HBA_ARCHIVE="$STARRING_CUTOVER_EVIDENCE/pre-authoring-writer-pg_hba.conf"
  AUTHORING_HBA_TEMP="$STARRING_PGDATA/.pg_hba.conf.authoring-${STARRING_CUTOVER_ID}"
  test ! -e "$LEGACY_HBA_ARCHIVE"
  test ! -e "$AUTHORING_HBA_TEMP"
  cp -p "$STARRING_PGDATA/pg_hba.conf" "$LEGACY_HBA_ARCHIVE"
  install -m 600 ops/postgres/staging-integrated-pg_hba.conf \
    "$AUTHORING_HBA_TEMP"
  cmp -s ops/postgres/staging-integrated-pg_hba.conf \
    "$AUTHORING_HBA_TEMP"
  ! cmp -s "$LEGACY_HBA_ARCHIVE" "$AUTHORING_HBA_TEMP"
  grep -F 'starring_authoring_session_writer' \
    "$AUTHORING_HBA_TEMP" >/dev/null
  mv "$AUTHORING_HBA_TEMP" "$STARRING_PGDATA/pg_hba.conf"
  sync
  /opt/homebrew/opt/postgresql@16/bin/pg_ctl \
    --pgdata "$STARRING_PGDATA" reload
  cmp -s "$STARRING_PGDATA/pg_hba.conf" \
    ops/postgres/staging-integrated-pg_hba.conf
  HBA_RELOAD_PROOF="$(
    starring_with_admin_pgpass /usr/bin/env \
      PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
      --no-psqlrc --set ON_ERROR_STOP=1 --no-password \
      --host 127.0.0.1 --port 5432 \
      --username "$STARRING_STAGING_CLUSTER_ADMIN" \
      --dbname postgres --tuples-only --no-align \
      --command "SELECT pg_catalog.concat_ws('|', pg_catalog.count(*) FILTER (WHERE error IS NOT NULL), pg_catalog.count(*)) FROM pg_catalog.pg_hba_file_rules"
  )"
  test "$HBA_RELOAD_PROOF" = '0|15'
)
```

An historical staging installation created before C1 has no runtime
interaction-token envelope keyring. It cannot rejoin this path until a
dedicated incremental provision has created the exact runtime item and the
current final verifier has accepted its semantic payload. Keep every service
stopped and do not rerun the one-shot fresh provisioner to backfill it. The
incremental mode rejects ambient `PG*` variables, makes no PostgreSQL
connection or mutation, requires both API keyrings to be valid and distinct,
and writes no secret through arguments, environment, logs, or output.

Run the keyring mode twice. The first call creates only the absent runtime
item. The second proves exact replay without rotation. Store only the outcome
and active key ID as evidence:

```zsh
(
  set -euo pipefail
  set +x
  env -i PATH="/usr/bin:/bin:/usr/sbin:/sbin" \
    "$STARRING_PROVISIONER_BINARY" \
    --provision-interaction-token-keyring \
    "$STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER" \
    "$STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT" \
    >"$STARRING_CUTOVER_EVIDENCE/interaction-keyring-created.txt"
  grep -E \
    '^outcome=created active_key_id=[A-Za-z0-9_-]+$' \
    "$STARRING_CUTOVER_EVIDENCE/interaction-keyring-created.txt" >/dev/null
  env -i PATH="/usr/bin:/bin:/usr/sbin:/sbin" \
    "$STARRING_PROVISIONER_BINARY" \
    --provision-interaction-token-keyring \
    "$STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER" \
    "$STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT" \
    >"$STARRING_CUTOVER_EVIDENCE/interaction-keyring-replay.txt"
  grep -E \
    '^outcome=exact_replay active_key_id=[A-Za-z0-9_-]+$' \
    "$STARRING_CUTOVER_EVIDENCE/interaction-keyring-replay.txt" >/dev/null
  test "$(cut -d= -f3 "$STARRING_CUTOVER_EVIDENCE/interaction-keyring-created.txt")" = \
    "$(cut -d= -f3 "$STARRING_CUTOVER_EVIDENCE/interaction-keyring-replay.txt")"
)
```

Any invalid pre-existing runtime item, invalid API keyring, different active
key ID on replay, or nonzero exit is a fail-closed incident. Preserve the item,
keep services stopped, and inspect only stable error codes. Never copy key
material or its hash into evidence.

Run the incremental mode twice. The first call must create exactly one
credential and the second must prove exact replay:

```zsh
(
  set -euo pipefail
  set +x
  env -i PATH="/usr/bin:/bin:/usr/sbin:/sbin" \
    "$STARRING_PROVISIONER_BINARY" \
    --provision-authoring-writer \
    "$STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER" \
    "$STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT" \
    >"$STARRING_CUTOVER_EVIDENCE/authoring-writer-created.txt"
  grep -Fx \
    'provisioned authoring_writer=created database=starring_runtime_staging credential_items=1 capabilities=5 snapshot_reader=v2_only' \
    "$STARRING_CUTOVER_EVIDENCE/authoring-writer-created.txt" >/dev/null
  env -i PATH="/usr/bin:/bin:/usr/sbin:/sbin" \
    "$STARRING_PROVISIONER_BINARY" \
    --provision-authoring-writer \
    "$STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER" \
    "$STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT" \
    >"$STARRING_CUTOVER_EVIDENCE/authoring-writer-replay.txt"
  grep -Fx \
    'provisioned authoring_writer=exact_replay database=starring_runtime_staging credential_items=1 capabilities=5 snapshot_reader=v2_only' \
    "$STARRING_CUTOVER_EVIDENCE/authoring-writer-replay.txt" >/dev/null
  /usr/bin/security find-generic-password \
    -s starring-api.staging \
    -a database.authoring-session-writer >/dev/null
  env -i PATH="/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin" \
    "$STARRING_PROVISIONER_BINARY" \
    --verify-final \
    "$STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER" \
    "$STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT" \
    >"$STARRING_CUTOVER_EVIDENCE/authoring-writer-final-verifier.txt"
  grep -Fx \
    'verified database=starring_runtime_staging application_database_credentials=20 keyrings=3 hba_rules=15' \
    "$STARRING_CUTOVER_EVIDENCE/authoring-writer-final-verifier.txt" >/dev/null
)
```

The incremental mode serializes concurrent attempts, reads only the existing
administrator credential, and writes only the authoring-writer item. One
serializable transaction creates the new role with its exact five functions
and atomically replaces the existing snapshot reader's v1 execute with v2
execute. It does not change any other role, credential, keyring, or ACL. Fresh
state is exactly absent writer role, absent writer item, and v1-only snapshot
reader access. Replay is exactly complete writer state and v2-only snapshot
reader access. Mixed, missing, or excess access fails closed. A missing
migration, old HBA, malformed replay, or asymmetric role and Keychain state
stops with a stable error code. A pre-commit failure restores the Keychain
item. A commit-indeterminate or failed compensating rollback preserves the
generated item for reconciliation instead of destroying the only matching
credential. A successful compensating rollback restores v1-only snapshot
access before dropping the writer and removing its Keychain item. The final
verifier receipt above is required before this historical path rejoins Gate 11;
the earlier nineteen-role verifier, if recorded, is pre-incremental evidence
only.

## Gate 11: prove the exact Keychain inventory without reading values

Fresh Gate 10, or the historical incremental path after its required final
verification, has created twenty application database URLs, one administrator
URL, two API keyrings, and one runtime interaction-token keyring while leaving
the three Discord provider items and the authoring-worker item unchanged.
Inventory only their existence. Never add `-w`, never export a Keychain value,
and never copy an access-control list or secret payload into evidence:

```zsh
(
  set -euo pipefail
  set +x
  COUNT=0
  for ENTRY in \
    "starring-api.staging:database.oauth-flow-writer" \
    "starring-api.staging:database.session-issuer" \
    "starring-api.staging:database.session-api" \
    "starring-api.staging:database.security-revoker" \
    "starring-api.staging:database.installation-authority-reader" \
    "starring-api.staging:database.authorized-snapshot-reader" \
    "starring-api.staging:database.promotion-executor" \
    "starring-api.staging:database.decision-reader" \
    "starring-api.staging:database.approval-executor" \
    "starring-api.staging:database.rejection-executor" \
    "starring-api.staging:database.apply-executor" \
    "starring-api.staging:database.cancellation-executor" \
    "starring-api.staging:database.deployment-status-reader" \
    "starring-api.staging:database.operational-deployment-status-reader" \
    "starring-api.staging:database.authoring-session-writer" \
    "starring-api.staging:discord.oauth-client-secret" \
    "starring-api.staging:discord.bot-token" \
    "starring-api.staging:keyring.product-action" \
    "starring-api.staging:keyring.snapshot-envelope" \
    "starring.runtime.staging:database.execution" \
    "starring.runtime.staging:database.exact-target" \
    "starring.runtime.staging:database.panel" \
    "starring.runtime.staging:database.serving" \
    "starring.runtime.staging:database.interaction" \
    "starring.runtime.staging:interaction.token-envelope-keyring" \
    "starring.runtime.staging:discord.bot-token" \
    "com.starring.llm-api-key:llm-api" \
    "starring.postgres.staging:database.cluster-admin"
  do
    SERVICE="${ENTRY%%:*}"
    ACCOUNT="${ENTRY#*:}"
    /usr/bin/security find-generic-password \
      -s "$SERVICE" -a "$ACCOUNT" >/dev/null
    COUNT=$(( COUNT + 1 ))
  done
  test "$COUNT" = 28
)
```

The provisioner has already semantically validated every generated URL, all
three keyrings, and both equal bot-token values. Application startup and the
final verifier perform independent semantic reads without printing them.
The runtime service now owns exactly seven Keychain accounts. Any later
rotation or complete removal must use the seven-account inventory and bounded
interaction-token keyring procedure in the runtime staging operations runbook;
never reuse the six-account pre-C1 checklist.

## Gate 12: build and install both services without starting them

Run the exact API
[Binary and LaunchAgent installation](./2026-07-19-production-control-plane-cutover.md#binary-and-launchagent-installation)
block with `STARRING_APPROVED_RELEASE_REVISION` set to the approved revision.
Do not run the later API `launchctl bootstrap` block.

Run the exact runtime
[Build and install an immutable revision](./2026-07-29-macos-starring-runtime-staging-operations.md#build-and-install-an-immutable-revision)
block from the same clean revision. Do not run the source runbook's combined
install-and-start block.

Install the runtime plist without starting it. The API installation block
copies the checked-in template, so materialize the installed API plist before
startup: restore the independently approved actual origin and Discord IDs,
override both OAuth return settings to the only deployed route `/v1/me`, and
set all three authoring references explicitly. Do not retain the template's
`/app` return path or infer actual IDs from that template:

```zsh
(
  set -euo pipefail
  set +x
  cd /Users/jungbogeon/starring
  mkdir -p "$HOME/Library/LaunchAgents" \
    "$HOME/Library/Logs/starring-api" \
    "$HOME/Library/Logs/starring-runtime"
  chmod 700 "$HOME/Library/Logs/starring-api" \
    "$HOME/Library/Logs/starring-runtime"
  install -m 600 ops/macos/local.starring.runtime.staging.plist \
    "$HOME/Library/LaunchAgents/local.starring.runtime.staging.plist"
  API_PLIST="$HOME/Library/LaunchAgents/local.starring.api.staging.plist"
  RUNTIME_PLIST="$HOME/Library/LaunchAgents/local.starring.runtime.staging.plist"
  test -f "$API_PLIST"
  /usr/libexec/PlistBuddy \
    -c "Set :EnvironmentVariables:STARRING_API_PUBLIC_ORIGIN ${STARRING_STAGING_PUBLIC_ORIGIN}" \
    "$API_PLIST"
  /usr/libexec/PlistBuddy \
    -c "Set :EnvironmentVariables:STARRING_API_DISCORD_APPLICATION_ID ${STARRING_DISCORD_APPLICATION_ID}" \
    "$API_PLIST"
  /usr/libexec/PlistBuddy \
    -c "Set :EnvironmentVariables:STARRING_API_DISCORD_BOT_USER_ID ${STARRING_DISCORD_BOT_USER_ID}" \
    "$API_PLIST"
  /usr/libexec/PlistBuddy \
    -c 'Set :EnvironmentVariables:STARRING_API_OAUTH_RETURN_PATHS_JSON [\"/v1/me\"]' \
    "$API_PLIST"
  /usr/libexec/PlistBuddy \
    -c 'Set :EnvironmentVariables:STARRING_API_OAUTH_DEFAULT_RETURN_PATH /v1/me' \
    "$API_PLIST"
  /usr/libexec/PlistBuddy \
    -c 'Set :EnvironmentVariables:STARRING_API_AUTHORING_SESSION_WRITER_DATABASE_SECRET_REFERENCE keychain:starring-api.staging:database.authoring-session-writer' \
    "$API_PLIST"
  /usr/libexec/PlistBuddy \
    -c 'Set :EnvironmentVariables:STARRING_API_AUTHORING_WORKER_URL http://127.0.0.1:18181' \
    "$API_PLIST"
  /usr/libexec/PlistBuddy \
    -c 'Set :EnvironmentVariables:STARRING_API_AUTHORING_WORKER_TOKEN_SECRET_REFERENCE keychain:com.starring.llm-api-key:llm-api' \
    "$API_PLIST"
  test "$(
    /usr/libexec/PlistBuddy \
      -c 'Print :EnvironmentVariables:STARRING_API_PUBLIC_ORIGIN' \
      "$API_PLIST"
  )" = "$STARRING_STAGING_PUBLIC_ORIGIN"
  test "$(
    /usr/libexec/PlistBuddy \
      -c 'Print :EnvironmentVariables:STARRING_API_DISCORD_APPLICATION_ID' \
      "$API_PLIST"
  )" = "$STARRING_DISCORD_APPLICATION_ID"
  test "$(
    /usr/libexec/PlistBuddy \
      -c 'Print :EnvironmentVariables:STARRING_API_DISCORD_BOT_USER_ID' \
      "$API_PLIST"
  )" = "$STARRING_DISCORD_BOT_USER_ID"
  test "$(
    /usr/libexec/PlistBuddy \
      -c 'Print :EnvironmentVariables:STARRING_API_OAUTH_RETURN_PATHS_JSON' \
      "$API_PLIST"
  )" = '["/v1/me"]'
  test "$(
    /usr/libexec/PlistBuddy \
      -c 'Print :EnvironmentVariables:STARRING_API_OAUTH_DEFAULT_RETURN_PATH' \
      "$API_PLIST"
  )" = '/v1/me'
  test "$(
    /usr/libexec/PlistBuddy \
      -c 'Print :EnvironmentVariables:STARRING_API_AUTHORING_SESSION_WRITER_DATABASE_SECRET_REFERENCE' \
      "$API_PLIST"
  )" = 'keychain:starring-api.staging:database.authoring-session-writer'
  test "$(
    /usr/libexec/PlistBuddy \
      -c 'Print :EnvironmentVariables:STARRING_API_AUTHORING_WORKER_URL' \
      "$API_PLIST"
  )" = 'http://127.0.0.1:18181'
  test "$(
    /usr/libexec/PlistBuddy \
      -c 'Print :EnvironmentVariables:STARRING_API_AUTHORING_WORKER_TOKEN_SECRET_REFERENCE' \
      "$API_PLIST"
  )" = 'keychain:com.starring.llm-api-key:llm-api'
  ! grep -F '/app' "$API_PLIST" >/dev/null
  ! grep -Eq 'REPLACE_WITH_|api\.example\.com' "$API_PLIST"
  plutil -lint "$API_PLIST"
  plutil -lint "$RUNTIME_PLIST"
  test -x "$HOME/.local/libexec/starring-api"
  test -x "$HOME/.local/libexec/starring-runtime"
  DOMAIN="gui/$(id -u)"
  ! launchctl print "$DOMAIN/local.starring.api.staging" >/dev/null 2>&1
  ! launchctl print "$DOMAIN/local.starring.runtime.staging" >/dev/null 2>&1
)
```

Record both binary SHA-256 values, both plist SHA-256 values, and the release
revision. These are non-secret release evidence.

## Gate 13: verify final credentials before starting clients

Compare the active HBA file with the reviewed manifest and keep every
application process stopped:

```zsh
(
  set -euo pipefail
  set +x
  cmp -s "$STARRING_PGDATA/pg_hba.conf" \
    ops/postgres/staging-integrated-pg_hba.conf
  DOMAIN="gui/$(id -u)"
  ! launchctl print "$DOMAIN/local.starring.api.staging" >/dev/null 2>&1
  ! launchctl print "$DOMAIN/local.starring.runtime.staging" >/dev/null 2>&1
  ! launchctl print "$DOMAIN/$STARRING_STAGING_TUNNEL_LABEL" >/dev/null 2>&1
)
```

Run the independent final verifier. It
reads the administrator and all twenty application URLs directly from
Keychain, proves exact TCP role and database identity for all twenty-one
connections, proves all application roles are direct `LOGIN` roles without
membership, strictly revalidates all three keyrings without emitting material,
and re-proves the
fifteen-rule HBA contract:

```zsh
(
  set -euo pipefail
  set +x
  env -i \
    PATH="/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin" \
    "$STARRING_PROVISIONER_BINARY" \
    --verify-final \
    "$STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER" \
    "$STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT" \
    >"$STARRING_CUTOVER_EVIDENCE/starring-staging-final-verifier.txt"
  grep -Fx \
    'verified database=starring_runtime_staging application_database_credentials=20 keyrings=3 hba_rules=15' \
    "$STARRING_CUTOVER_EVIDENCE/starring-staging-final-verifier.txt" >/dev/null
)
```

### Post-final quarantine procedure

After Gate 10A, the peer Gate 9 transport no longer exists. For any activation
failure or rollback while the final cluster is reachable, unload the Starring
tunnel first, then both applications, and then run this API-first, runtime-last
quarantine through the administrator Keychain URL:

```zsh
(
  set -euo pipefail
  set +x
  cd /Users/jungbogeon/starring
  DOMAIN="gui/$(id -u)"
  for LABEL in \
    "$STARRING_STAGING_TUNNEL_LABEL" \
    local.starring.api.staging \
    local.starring.runtime.staging
  do
    SERVICE="$DOMAIN/$LABEL"
    if launchctl print "$SERVICE" >/dev/null 2>&1
    then
      launchctl bootout "$SERVICE"
    fi
    ! launchctl print "$SERVICE" >/dev/null 2>&1
  done
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  starring_with_admin_pgpass /usr/bin/env \
    PGOPTIONS="-c starring.expected_staging_database=starring_runtime_staging -c starring.expected_staging_system_identifier=${STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER}" \
    PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
      --no-psqlrc --set ON_ERROR_STOP=1 --no-password \
      --host 127.0.0.1 --port 5432 \
      --username "$STARRING_STAGING_CLUSTER_ADMIN" \
      --dbname starring_runtime_staging \
      --file ops/postgres/staging-api-role-bootstrap.sql
  unset PGOPTIONS
  starring_with_admin_pgpass /usr/bin/env \
    PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
    --no-psqlrc --set ON_ERROR_STOP=1 --no-password \
    --host 127.0.0.1 --port 5432 \
    --username "$STARRING_STAGING_CLUSTER_ADMIN" \
    --set runtime_enable=off \
    --set expected_database=starring_runtime_staging \
    --set expected_system_identifier="$STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER" \
    --set runtime_dedicated_cluster_acknowledgement="$STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT" \
    --dbname starring_runtime_staging \
    --command "SELECT 1 / CASE WHEN NOT EXISTS (SELECT 1 FROM pg_catalog.pg_stat_activity WHERE pid <> pg_catalog.pg_backend_pid() AND (backend_type = 'client backend' OR usesysid IN (pg_catalog.to_regrole('starring_runtime_execution'), pg_catalog.to_regrole('starring_runtime_exact_target'), pg_catalog.to_regrole('starring_runtime_panel'), pg_catalog.to_regrole('starring_runtime_serving'), pg_catalog.to_regrole('starring_runtime_interaction')))) AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_prepared_xacts) THEN 1 ELSE 0 END AS cluster_quiescence_proof" \
    --file ops/postgres/staging-runtime-role-bootstrap.sql
)
```

After Gate 10A, any nonzero exit through the end of both service-readiness
gates is an activation failure. Immediately run the post-final quarantine
procedure. Quarantine clears all twenty passwords and the peer path is
already gone, so continue to physical rollback. There is no in-place
reprovision retry after Gate 10A.

## Gate 14: combined negative probes

First prove all twenty roles are direct logins, have no membership or
ownership, and lack database and schema creation:

```zsh
(
  set -euo pipefail
  set +x
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  ROLE_PROOF="$(
    starring_with_admin_pgpass /usr/bin/env \
      PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
      --no-psqlrc --set ON_ERROR_STOP=1 --no-password \
      --host 127.0.0.1 --port 5432 \
      --username "$STARRING_STAGING_CLUSTER_ADMIN" \
      --dbname starring_runtime_staging --tuples-only --no-align \
      --command "
        WITH managed(role_name) AS (
          VALUES
            ('starring_identity_oauth'),
            ('starring_identity_issuer'),
            ('starring_identity_session'),
            ('starring_identity_security'),
            ('starring_installation_authority_reader'),
            ('starring_authorized_snapshot_reader'),
            ('starring_promotion_executor'),
            ('starring_decision_reader'),
            ('starring_decision_approval'),
            ('starring_decision_rejection'),
            ('starring_decision_apply'),
            ('starring_decision_cancellation'),
            ('starring_deployment_status_reader'),
            ('starring_operational_deployment_status_reader'),
            ('starring_authoring_session_writer'),
            ('starring_runtime_execution'),
            ('starring_runtime_exact_target'),
            ('starring_runtime_panel'),
            ('starring_runtime_serving'),
            ('starring_runtime_interaction')
        ),
        roles AS (
          SELECT role.oid, role.rolname, role.rolcanlogin
          FROM managed
          JOIN pg_catalog.pg_roles AS role
            ON role.rolname = managed.role_name
        )
        SELECT pg_catalog.concat_ws(
          '|',
          (SELECT pg_catalog.count(*) FROM roles),
          (SELECT pg_catalog.count(*) FROM roles WHERE rolcanlogin),
          (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_auth_members AS membership
            WHERE membership.member IN (SELECT oid FROM roles)
               OR membership.roleid IN (SELECT oid FROM roles)
          ),
          (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_shdepend AS dependency
            WHERE dependency.refclassid = 'pg_catalog.pg_authid'::REGCLASS
              AND dependency.refobjid IN (SELECT oid FROM roles)
              AND dependency.deptype = 'o'
          ),
          (
            SELECT pg_catalog.count(*)
            FROM roles
            WHERE pg_catalog.has_database_privilege(
                    rolname,
                    'starring_runtime_staging',
                    'CREATE'
                  )
               OR pg_catalog.has_database_privilege(
                    rolname,
                    'starring_runtime_staging',
                    'TEMPORARY'
                  )
               OR pg_catalog.has_schema_privilege(
                    rolname,
                    'public',
                    'CREATE'
                  )
          )
        )
      "
  )"
  test "$ROLE_PROOF" = '20|20|0|0|0'
)
```

The Gate 13 final verifier already proved every correct credential directly
against the target. For every role, independently prove that a deliberately
wrong password reaches SCRAM on the target while the same request is rejected
by HBA before authentication on `postgres`:

```zsh
(
  set -euo pipefail
  set +x
  ROLES=(
    starring_identity_oauth
    starring_identity_issuer
    starring_identity_session
    starring_identity_security
    starring_installation_authority_reader
    starring_authorized_snapshot_reader
    starring_promotion_executor
    starring_decision_reader
    starring_decision_approval
    starring_decision_rejection
    starring_decision_apply
    starring_decision_cancellation
    starring_deployment_status_reader
    starring_operational_deployment_status_reader
    starring_authoring_session_writer
    starring_runtime_execution
    starring_runtime_exact_target
    starring_runtime_panel
    starring_runtime_serving
    starring_runtime_interaction
  )
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  for ROLE in "${ROLES[@]}"
  do
    ERROR_PATH="$STARRING_CUTOVER_EVIDENCE/wrong-password-${ROLE}.txt"
    if LC_ALL=C PGSSLMODE=disable \
      PGPASSWORD='starring-invalid-password-probe' \
      /opt/homebrew/opt/postgresql@16/bin/psql \
        --no-psqlrc --set ON_ERROR_STOP=1 \
        --host 127.0.0.1 --port 5432 \
        --username "$ROLE" \
        --dbname starring_runtime_staging \
        --command "SELECT 1" >/dev/null 2>"$ERROR_PATH"
    then
      exit 1
    fi
    grep -F "password authentication failed for user \"$ROLE\"" \
      "$ERROR_PATH" >/dev/null
    ERROR_PATH="$STARRING_CUTOVER_EVIDENCE/wrong-database-${ROLE}.txt"
    if LC_ALL=C PGSSLMODE=disable \
      PGPASSWORD='starring-invalid-password-probe' \
      /opt/homebrew/opt/postgresql@16/bin/psql \
        --no-psqlrc --set ON_ERROR_STOP=1 \
        --host 127.0.0.1 --port 5432 \
        --username "$ROLE" \
        --dbname postgres \
        --command "SELECT 1" >/dev/null 2>"$ERROR_PATH"
    then
      exit 1
    fi
    grep -F 'pg_hba.conf rejects connection' "$ERROR_PATH" >/dev/null
    grep -F 'database "postgres"' "$ERROR_PATH" >/dev/null
  done
  unset PGPASSWORD
)
```

The final verifier supersedes the runtime runbook's prompt-oriented
authentication loop. Gate 15 still requires the runtime's deep readiness
functions. Never reinstall the runtime-only HBA manifest.

Create one temporary future database and grant `PUBLIC` connectivity. The
final integrated HBA must still deny all twenty roles:

```zsh
(
  set -euo pipefail
  set +x
  PROBE_DATABASE=starring_integrated_hba_probe
  unset PGAPPNAME PGDATABASE PGHOST PGHOSTADDR PGOPTIONS PGPASSFILE
  unset PGPASSWORD PGPORT PGSSLCERT PGSSLKEY PGSSLMODE PGSSLROOTCERT PGUSER
  starring_with_admin_pgpass /usr/bin/env \
    PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
    --no-psqlrc --set ON_ERROR_STOP=1 --no-password \
    --host 127.0.0.1 --port 5432 \
    --username "$STARRING_STAGING_CLUSTER_ADMIN" \
    --dbname postgres \
    --command "CREATE DATABASE ${PROBE_DATABASE}" \
    --command "GRANT CONNECT, TEMPORARY ON DATABASE ${PROBE_DATABASE} TO PUBLIC"
  ROLES=(
    starring_identity_oauth
    starring_identity_issuer
    starring_identity_session
    starring_identity_security
    starring_installation_authority_reader
    starring_authorized_snapshot_reader
    starring_promotion_executor
    starring_decision_reader
    starring_decision_approval
    starring_decision_rejection
    starring_decision_apply
    starring_decision_cancellation
    starring_deployment_status_reader
    starring_operational_deployment_status_reader
    starring_authoring_session_writer
    starring_runtime_execution
    starring_runtime_exact_target
    starring_runtime_panel
    starring_runtime_serving
    starring_runtime_interaction
  )
  for ROLE in "${ROLES[@]}"
  do
    ERROR_PATH="$STARRING_CUTOVER_EVIDENCE/future-database-${ROLE}.txt"
    if LC_ALL=C PGSSLMODE=disable \
      PGPASSWORD='starring-invalid-password-probe' \
      /opt/homebrew/opt/postgresql@16/bin/psql \
        --no-psqlrc --set ON_ERROR_STOP=1 \
        --host 127.0.0.1 --port 5432 \
        --username "$ROLE" \
        --dbname "$PROBE_DATABASE" \
        --command "SELECT 1" >/dev/null 2>"$ERROR_PATH"
    then
      exit 1
    fi
    grep -F 'pg_hba.conf rejects connection' "$ERROR_PATH" >/dev/null
    grep -F "database \"$PROBE_DATABASE\"" "$ERROR_PATH" >/dev/null
  done
  starring_with_admin_pgpass /usr/bin/env \
    PGSSLMODE=disable /opt/homebrew/opt/postgresql@16/bin/psql \
    --no-psqlrc --set ON_ERROR_STOP=1 --no-password \
    --host 127.0.0.1 --port 5432 \
    --username "$STARRING_STAGING_CLUSTER_ADMIN" \
    --dbname postgres \
    --command "DROP DATABASE ${PROBE_DATABASE}"
)
```

If any probe fails after the probe database is created, run the post-final
quarantine procedure before dropping the probe database, then continue to
physical rollback. Do not leave application roles login-capable under an
unproven HBA boundary.

## Gate 15: start runtime, then API, with public ingress closed

Start the runtime first:

```zsh
(
  set -euo pipefail
  set +x
  DOMAIN="gui/$(id -u)"
  INGRESS_SERVICE="$DOMAIN/$STARRING_STAGING_TUNNEL_LABEL"
  SERVICE="$DOMAIN/local.starring.runtime.staging"
  PLIST="$HOME/Library/LaunchAgents/local.starring.runtime.staging.plist"
  ! launchctl print "$INGRESS_SERVICE" >/dev/null 2>&1
  ! launchctl print "$SERVICE" >/dev/null 2>&1
  launchctl enable "$SERVICE"
  launchctl bootstrap "$DOMAIN" "$PLIST"
  READY=0
  for ATTEMPT in {1..60}
  do
    if curl --fail --silent --show-error --max-time 1 \
      http://127.0.0.1:19091/health/live >/dev/null 2>&1 \
      && curl --fail --silent --show-error --max-time 1 \
        http://127.0.0.1:19091/health/ready >/dev/null 2>&1
    then
      READY=1
      break
    fi
    sleep 1
  done
  test "$READY" = 1
  lsof -nP -iTCP:19091 -sTCP:LISTEN \
    >"$STARRING_CUTOVER_EVIDENCE/runtime-listener.txt"
  grep -F '127.0.0.1:19091' \
    "$STARRING_CUTOVER_EVIDENCE/runtime-listener.txt" >/dev/null
  launchctl print "$SERVICE" \
    >"$STARRING_CUTOVER_EVIDENCE/runtime-launchctl-accepted.txt"
)
```

Run the exact runtime
[SIGTERM acceptance](./2026-07-29-macos-starring-runtime-staging-operations.md#sigterm-acceptance)
block. It must restart the same installed revision and return to readiness
before the API starts.

Then start the API:

```zsh
(
  set -euo pipefail
  set +x
  DOMAIN="gui/$(id -u)"
  INGRESS_SERVICE="$DOMAIN/$STARRING_STAGING_TUNNEL_LABEL"
  SERVICE="$DOMAIN/local.starring.api.staging"
  PLIST="$HOME/Library/LaunchAgents/local.starring.api.staging.plist"
  API_LOG="$HOME/Library/Logs/starring-api/runtime.log"
  if test -f "$API_LOG"
  then
    API_LOG_OFFSET="$(wc -c <"$API_LOG" | tr -d ' ')"
  else
    API_LOG_OFFSET=0
  fi
  ! launchctl print "$INGRESS_SERVICE" >/dev/null 2>&1
  ! launchctl print "$SERVICE" >/dev/null 2>&1
  launchctl enable "$SERVICE"
  launchctl bootstrap "$DOMAIN" "$PLIST"
  READY=0
  for ATTEMPT in {1..90}
  do
    if curl --fail --silent --show-error --max-time 1 \
      --header "Host: $STARRING_STAGING_PUBLIC_HOST" \
      http://127.0.0.1:18080/health/live >/dev/null 2>&1 \
      && curl --fail --silent --show-error --max-time 1 \
        --header "Host: $STARRING_STAGING_PUBLIC_HOST" \
        http://127.0.0.1:18080/health/ready >/dev/null 2>&1
    then
      READY=1
      break
    fi
    sleep 1
  done
  test "$READY" = 1
  lsof -nP -iTCP:18080 -sTCP:LISTEN \
    >"$STARRING_CUTOVER_EVIDENCE/api-listener.txt"
  grep -F '127.0.0.1:18080' \
    "$STARRING_CUTOVER_EVIDENCE/api-listener.txt" >/dev/null
  launchctl print "$SERVICE" \
    >"$STARRING_CUTOVER_EVIDENCE/api-launchctl-accepted.txt"
  /usr/bin/tail -c "+$(( API_LOG_OFFSET + 1 ))" "$API_LOG" \
    | grep -Fx 'starring_api_authoring_status=ready' \
    >"$STARRING_CUTOVER_EVIDENCE/api-authoring-startup-status.txt"
  test "$(
    wc -l <"$STARRING_CUTOVER_EVIDENCE/api-authoring-startup-status.txt" \
      | tr -d ' '
  )" = 1
  ! /usr/bin/tail -c "+$(( API_LOG_OFFSET + 1 ))" "$API_LOG" \
    | grep -F 'starring_api_authoring_status=unavailable' >/dev/null
  ! launchctl print "$INGRESS_SERVICE" >/dev/null 2>&1
)
```

API startup evidence must combine aggregate fourteen-core-pool readiness with
the separately successful fifteenth authoring-writer and loopback-worker
preflights described in
[Startup and deep-readiness proof](./2026-07-19-production-control-plane-cutover.md#startup-and-deep-readiness-proof).
Runtime readiness must remain the empty-open contract described in the runtime
runbook. Review only finite redacted status lines from both logs. Never copy a
database URL, OAuth code, Discord token, Keychain value, or key material into
evidence.

The Starring tunnel remains unloaded at the end of this runbook. A separate
edge change must prove Cloudflare Access, OAuth-start rate control, exact
path-only routing, callback-query redaction, and local health-route exclusion
before loading it. Never route `19091`, `/health/live`, or `/health/ready`.

## Acceptance record

The cutover is accepted only when a change record contains all of these facts:

- approved revision and installed API/runtime SHA-256 values;
- old PGDATA archive path, old control-data receipt, and old configuration
  hashes;
- new offline system identifier and independently approved v2 acknowledgement;
- new PostgreSQL 16.14, checksum, SCRAM, administrator contract, and listener
  proofs;
- exact seven-rule bootstrap HBA and ident proof, final fifteen-rule HBA
  proof, peer-removal proof, and physical replication rejection;
- immutable bootstrap and provisioner binary SHA-256 values;
- embedded bootstrap receipt with 178 relations and 124 capability functions;
- exact migration ledger diff with no difference;
- exact database, `public` schema, ledger, relation, routine, and type ownership
  proof plus owner zero-membership postflight and migrator absence;
- API quarantine and runtime quarantine exit status;
- fresh-path one-shot provisioner receipt and aggregate `20|20|20` verifier
  proof without verifier values; for the historical path, retain the
  pre-incremental nineteen-role receipt and instead record the legacy HBA
  archive, atomic replacement and reload proof, writer-created receipt,
  exact-replay receipt, and twenty-application-credential final verifier;
- Keychain existence count `28`, Discord provider credential versions,
  authoring-worker item existence, and generated key IDs only;
- runtime enable, API enable, and final
  twenty-one-connection/three-keyring verifier exit status;
- aggregate application-role proof `20|20|0|0|0` and combined negative-probe
  exit status;
- installed API plist proof for the unchanged approved origin and Discord IDs,
  OAuth return allowlist and default `/v1/me`, and all three exact authoring
  references;
- runtime listener, readiness, and SIGTERM acceptance;
- API listener, fourteen-core-pool readiness, and the current process's exact
  `starring_api_authoring_status=ready` startup line;
- confirmation that `local.cloudflared.starring` remains unloaded;
- statement that no customer route or customer guild was exercised.

Until every line is present, describe the state as `cutover incomplete`,
unload both services, and quarantine both role sets.

## Rollback

Rollback is a physical cluster rollback. The old cluster remains available
because Gate 2 renamed rather than deleted it. Migrations are forward-only; do
not improvise reverse SQL.

First unload tunnel, API, and runtime. Before Gate 10A, use the peer Gate 9
quarantine. After Gate 10A, use the post-final quarantine procedure. If the
applicable quarantine cannot run, record that failure and continue only under
the incident owner because the final HBA, service unload, and PostgreSQL stop
become the remaining containment boundary.

```zsh
(
  set -euo pipefail
  set +x
  DOMAIN="gui/$(id -u)"
  for LABEL in \
    "$STARRING_STAGING_TUNNEL_LABEL" \
    local.starring.api.staging \
    local.starring.runtime.staging
  do
    SERVICE="$DOMAIN/$LABEL"
    if launchctl print "$SERVICE" >/dev/null 2>&1
    then
      launchctl bootout "$SERVICE"
    fi
    ! launchctl print "$SERVICE" >/dev/null 2>&1
  done
  brew services stop postgresql@16
  for ATTEMPT in {1..60}
  do
    if ! lsof -nP -iTCP:5432 -sTCP:LISTEN >/dev/null 2>&1
    then
      break
    fi
    sleep 1
  done
  ! lsof -nP -iTCP:5432 -sTCP:LISTEN >/dev/null 2>&1
  ! lsof -nP -iTCP:18080 -sTCP:LISTEN >/dev/null 2>&1
  ! lsof -nP -iTCP:19091 -sTCP:LISTEN >/dev/null 2>&1
)
```

Archive the failed new cluster without deletion, then restore the exact old
directory:

```zsh
(
  set -euo pipefail
  set +x
  FAILED_PGDATA="${STARRING_PGDATA}.failed-${STARRING_CUTOVER_ID}"
  test -d "$STARRING_OLD_PGDATA_ARCHIVE"
  test ! -e "$FAILED_PGDATA"
  if test -e "$STARRING_PGDATA"
  then
    test -d "$STARRING_PGDATA"
    test ! -e "$STARRING_PGDATA/postmaster.pid"
    if test -f "$STARRING_PGDATA/global/pg_control"
    then
      LC_ALL=C /opt/homebrew/opt/postgresql@16/bin/pg_controldata \
        "$STARRING_PGDATA" \
        >"$STARRING_CUTOVER_EVIDENCE/failed-new-pg-controldata.txt"
    else
      print -r -- 'new cluster has no readable pg_control' \
        >"$STARRING_CUTOVER_EVIDENCE/failed-new-pgdata-incomplete.txt"
    fi
    sync
    mv "$STARRING_PGDATA" "$FAILED_PGDATA"
    test -d "$FAILED_PGDATA"
  else
    print -r -- 'new cluster path was never created' \
      >"$STARRING_CUTOVER_EVIDENCE/failed-new-pgdata-absent.txt"
  fi
  mv "$STARRING_OLD_PGDATA_ARCHIVE" "$STARRING_PGDATA"
  sync
  test -d "$STARRING_PGDATA"
  test "$(tr -d '[:space:]' <"$STARRING_PGDATA/PG_VERSION")" = 16
  LC_ALL=C /opt/homebrew/opt/postgresql@16/bin/pg_controldata \
    "$STARRING_PGDATA" \
    >"$STARRING_CUTOVER_EVIDENCE/restored-old-pg-controldata.txt"
  diff -u \
    "$STARRING_CUTOVER_EVIDENCE/pre-cutover/old-pg-controldata.txt" \
    "$STARRING_CUTOVER_EVIDENCE/restored-old-pg-controldata.txt"
)
```

The old cluster was never started between the entry receipt and this restore,
so the control-data files must be identical. Any diff is a blocker; never
weaken or filter this comparison.

Restore the old service artifacts only after the rollback owner restores the
previous Keychain payloads from the external password manager:

```zsh
(
  set -euo pipefail
  set +x
  PREVIOUS="$STARRING_CUTOVER_EVIDENCE/pre-cutover"
  FAILED_ARTIFACTS="$STARRING_CUTOVER_EVIDENCE/failed-new-artifacts"
  mkdir -p "$HOME/.local/libexec" "$HOME/Library/LaunchAgents"
  mkdir -m 700 "$FAILED_ARTIFACTS"
  for ARTIFACT in \
    "$HOME/.local/libexec/starring-api" \
    "$HOME/Library/LaunchAgents/local.starring.api.staging.plist" \
    "$HOME/Library/LaunchAgents/local.starring.runtime.staging.plist"
  do
    if test -f "$ARTIFACT"
    then
      mv "$ARTIFACT" "$FAILED_ARTIFACTS/$(basename "$ARTIFACT")"
    fi
  done
  if test -L "$HOME/.local/libexec/starring-runtime"
  then
    mv "$HOME/.local/libexec/starring-runtime" \
      "$FAILED_ARTIFACTS/starring-runtime"
  fi
  if test -f "$PREVIOUS/starring-api"
  then
    install -m 500 "$PREVIOUS/starring-api" \
      "$HOME/.local/libexec/starring-api"
  fi
  if test -f "$PREVIOUS/local.starring.api.staging.plist"
  then
    install -m 600 "$PREVIOUS/local.starring.api.staging.plist" \
      "$HOME/Library/LaunchAgents/local.starring.api.staging.plist"
  fi
  if test -f "$PREVIOUS/local.starring.runtime.staging.plist"
  then
    install -m 600 "$PREVIOUS/local.starring.runtime.staging.plist" \
      "$HOME/Library/LaunchAgents/local.starring.runtime.staging.plist"
  fi
  if test -f "$PREVIOUS/runtime-link-target.txt"
  then
    PREVIOUS_RUNTIME_TARGET="$(
      tr -d '\r\n' <"$PREVIOUS/runtime-link-target.txt"
    )"
    print -r -- "$PREVIOUS_RUNTIME_TARGET" \
      | grep -Eq '^starring-runtime-[0-9a-f]{40}$'
    test -x "$HOME/.local/libexec/$PREVIOUS_RUNTIME_TARGET"
    ln -sfn "$PREVIOUS_RUNTIME_TARGET" \
      "$HOME/.local/libexec/starring-runtime"
  fi
  for PLIST in \
    "$HOME/Library/LaunchAgents/local.starring.api.staging.plist" \
    "$HOME/Library/LaunchAgents/local.starring.runtime.staging.plist"
  do
    if test -f "$PLIST"
    then
      plutil -lint "$PLIST"
    fi
  done
)
```

Start the restored old PostgreSQL service only when the entry receipt says it
was started:

```zsh
(
  set -euo pipefail
  set +x
  ENTRY_STATE="$STARRING_CUTOVER_EVIDENCE/entry-state.env"
  grep -Fx 'postgresql_was_started=true' "$ENTRY_STATE" >/dev/null
  brew services start postgresql@16
  READY=0
  for ATTEMPT in {1..60}
  do
    if /opt/homebrew/opt/postgresql@16/bin/pg_isready \
      --host 127.0.0.1 --port 5432 >/dev/null 2>&1
    then
      READY=1
      break
    fi
    sleep 1
  done
  test "$READY" = 1
)
```

Do not automatically restore API, runtime, or tunnel merely because an entry
receipt says it was loaded. First prove the restored binary is compatible with
the restored old migration ledger and that its previous Keychain credentials
have been restored. Then restore only the previously loaded service, in the
order runtime, API, tunnel, with its original health and edge checks.

Do not delete the failed new cluster, the old-cluster evidence, the
new administrator/application/keyring Keychain items, or any retained
provider item until the rollback owner closes the rollback window. Secret
deletion and archive retention are separate reviewed follow-up operations.

## End state

Authoring this document does not change the host. Before execution, the only
truthful status is:

```text
integrated staging cutover: not executed
database cutover evidence: absent
application activation evidence: absent
public ingress: not authorized by this runbook
```
