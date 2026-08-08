# Production Control Plane Cutover Runbook

## Status

This runbook describes the fail-closed migration and maintenance contract for
the production control plane. It does not authorize production cutover until
the database-role, RLS, capability-probe, HTTP composition, runtime Live,
failure-cohort, backup/restore, merge-candidate, and merged-main gates in the
accepted design are implemented and green.

The current `starring-api` is a staging control plane. It can authenticate
product users and carry authoring, promotion, approval, rejection, Apply, and
status requests through the verified control boundary. The staging runtime can
converge an exact deployment to Live and serve the supported recipe; B6 proved
one bounded standing-fixture path. D1 restart and source-level injected-failure
cohorts are complete. The D2 disposable-guild sequence, exact D3
merge-candidate gate, and merged-main certification remain incomplete. Do not
connect this slice to a customer guild or advertise
commercial production automation. The current continuation state is recorded
in the
[Commercial certification Phase D handoff](../handoffs/2026-08-01-commercial-certification-phase-d-handoff.md).

The current repository contract is 125 ordered migrations through
`202608040004_refresh_serving_pending_product_drain_readiness_v1.sql`, 198
owned user-schema relations, and 137 capability functions. Dated D1 evidence at
117 migrations and 135 functions remains historical and must not be rewritten
or used as evidence for the current candidate.

## Required operators and credentials

- `starring_migrator` performs schema migration and ownership handoff.
- `starring_api`, `starring_runtime`, and `starring_maintenance` remain stopped
  until their startup capability probes succeed.
- Product identity uses four distinct direct-login credentials: the OAuth flow
  writer, session issuer, session API, and security revoker. Do not reuse one
  login or pool for more than one of these capabilities.
- The A4/A5 API process uses fifteen distinct direct-login database
  credentials in total: fourteen core product-control credentials plus one
  isolated authoring-session writer credential. The core capabilities beyond
  product identity are installation-authority read, authorized-snapshot read,
  promotion execution, decision read, approval execution, rejection execution,
  Apply execution, lifecycle-cancellation execution, deployment-status read,
  and operational-deployment-status read. The fifteenth credential may execute
  only the authoring writer allowlist and is never reused by a reader, decision,
  promotion, or identity adapter. All fifteen connect to one logical database
  under different roles. The fourteen core roles are checked as one mandatory
  topology before the process becomes ready; the authoring role is checked
  independently before authoring admission is composed.
- `starring_owner` is `NOLOGIN` and is never used by an application process.
- Migration, API, the four product-identity roles, runtime, and maintenance
  credentials are separate secret references. They are never passed as
  command-line literals or committed.

The reviewed staging role manifest uses these exact direct-login role names:

| Capability | Role |
| --- | --- |
| OAuth flow writer | `starring_identity_oauth` |
| Session issuer | `starring_identity_issuer` |
| Session API | `starring_identity_session` |
| Security revoker | `starring_identity_security` |
| Installation-authority reader | `starring_installation_authority_reader` |
| Authorized-snapshot reader | `starring_authorized_snapshot_reader` |
| Promotion executor | `starring_promotion_executor` |
| Decision reader | `starring_decision_reader` |
| Approval executor | `starring_decision_approval` |
| Rejection executor | `starring_decision_rejection` |
| Apply executor | `starring_decision_apply` |
| Lifecycle-cancellation executor | `starring_decision_cancellation` |
| Deployment-status reader | `starring_deployment_status_reader` |
| Operational-status reader | `starring_operational_deployment_status_reader` |
| Authoring-session writer | `starring_authoring_session_writer` |

### Staging database role bootstrap

The executable fifteen-role manifests are
`ops/postgres/staging-api-role-bootstrap.sql` and
`ops/postgres/staging-api-role-enable.sql`. The component grant snippets later
in this runbook explain individual contracts; they are not substitutes for
these manifests. Both files are restricted to a dedicated disposable staging
PostgreSQL cluster and database. The database name must contain a `staging`
segment after `starring`, and the session must independently name the same
database through `starring.expected_staging_database`. The session must also
match the PostgreSQL control-system identifier from the reviewed infrastructure
inventory through `starring.expected_staging_system_identifier`. Record the
host, port, database, administrator, and system identifier in that inventory
before the maintenance window. Never derive the expected identifier from the
target as part of either execution command. Never run either manifest on a
shared cluster, a production database, or a customer-data clone. The local
Homebrew PostgreSQL cluster inspected on 2026-07-20 is ineligible because it
contains shared test databases and accepts trusted local and loopback clients.

`starring_owner` must exist before the first application migration, and every
application relation and capability function must be created by that owner.
Create it once from an interactive cluster-administrator session without a
password, grant the migrator only the temporary membership needed to `SET ROLE
starring_owner`, apply migrations under that role, and revoke the membership
afterward. The post-migration manifest verifies the owner and function
ownership; it does not guess at or silently repair an incorrect ownership
history.

```sql
CREATE ROLE starring_owner
    NOLOGIN
    NOSUPERUSER
    NOCREATEDB
    NOCREATEROLE
    NOINHERIT
    NOREPLICATION
    NOBYPASSRLS
    CONNECTION LIMIT 0;
```

Before touching roles, stop the API, tunnel, migration process, schedulers, and
every other database client. Isolate the dedicated cluster at the network
boundary. Configure `pg_hba.conf` so a reviewed administrator rule and one
first-match application rule cover only the exact staging database, the
fifteen exact request roles, and the exact application source address. Use
`scram-sha-256` for local and network password authentication and `hostssl` for
network traffic. Put explicit reject rules after those allow rules for every
other database, role, and source path. `trust`, `peer`, and `ident` are not
permitted paths for a request role. Reload the configuration, inspect
`pg_hba_file_rules` for parse errors, order, database, role list, source address,
and authentication method, then prove an unlisted role and an unlisted source
cannot connect. Any ambiguous or broader earlier match stops the procedure.

After every migration and migration-specific preflight is green, run the
bootstrap manifest as the dedicated staging cluster administrator.
`ON_ERROR_STOP` is mandatory. The file sets transaction-local lock, statement,
idle-transaction, and search-path bounds. Its first transaction commits a
fail-closed quarantine before function validation: all managed roles become
`NOLOGIN`, their passwords and settings are cleared, memberships are removed,
and direct database, schema, relation, column, sequence, routine, parameter,
and default privileges are reconciled. The second transaction drains every
client session in the dedicated cluster, rejects prepared transactions,
verifies the owner and all 53 API capability functions, grants the exact
request capabilities,
and leaves all request roles quarantined as `NOLOGIN` with null passwords. If
the second transaction fails, the first transaction remains committed; keep
staging offline and repair the contract before rerunning it.

```bash
STAGING_DATABASE=starring_staging
STAGING_DATABASE_HOST=replace_with_staging_database_host
STAGING_DATABASE_PORT=5432
STAGING_CLUSTER_ADMIN=replace_with_staging_cluster_admin
STAGING_SYSTEM_IDENTIFIER=replace_with_reviewed_staging_system_identifier
PGOPTIONS="-c starring.expected_staging_database=$STAGING_DATABASE -c starring.expected_staging_system_identifier=$STAGING_SYSTEM_IDENTIFIER" \
  psql --no-psqlrc --set ON_ERROR_STOP=1 --password \
    --host "$STAGING_DATABASE_HOST" --port "$STAGING_DATABASE_PORT" \
    --dbname "$STAGING_DATABASE" --username "$STAGING_CLUSTER_ADMIN" \
    --file ops/postgres/staging-api-role-bootstrap.sql
unset STAGING_SYSTEM_IDENTIFIER STAGING_CLUSTER_ADMIN \
  STAGING_DATABASE_PORT STAGING_DATABASE_HOST STAGING_DATABASE
```

The bootstrap creates any missing request roles but does not enable login. It
also removes legacy `starring_api` capabilities and grants exactly database
`CONNECT`, `public` schema `USAGE`, and the 53 reviewed function identities.
Every rerun is fail-closed: it returns all fifteen request roles to quarantine
and clears every password before validating capabilities. PostgreSQL preserves
some database, schema, and object grants issued by an alternate grantor when a
cluster administrator performs an ordinary revoke. The manifest detects the
remaining effective or public capability and stops after quarantine instead of
claiming automatic repair. Inspect the affected catalog ACL with `aclexplode`,
identify the recorded grantor, and use a separately reviewed transaction that
sets the local role to that grantor, revokes the exact privilege with `CASCADE`,
and resets the role. Review downstream revocation impact before execution, do
not use object-dropping shortcuts, and rerun the full bootstrap afterward.

Next prove `SHOW password_encryption` returns `scram-sha-256`. While the API,
tunnel, and all other clients remain stopped, assign fifteen distinct,
password-manager-generated values in the same interactive administrator
`psql` session. Use the client-side prompt commands below; each command prompts
twice and keeps the secret out of SQL text, arguments, shell history, logs, and
this repository. The roles remain `NOLOGIN` throughout this operation.

```text
\password starring_identity_oauth
\password starring_identity_issuer
\password starring_identity_session
\password starring_identity_security
\password starring_installation_authority_reader
\password starring_authorized_snapshot_reader
\password starring_promotion_executor
\password starring_decision_reader
\password starring_decision_approval
\password starring_decision_rejection
\password starring_decision_apply
\password starring_decision_cancellation
\password starring_deployment_status_reader
\password starring_operational_deployment_status_reader
\password starring_authoring_session_writer
```

Do not use `ALTER ROLE ... PASSWORD '...'`, reuse a value between roles, put a
password-bearing database URL in an argument, or generate a SQL file containing
secrets. Verify only the aggregate result without printing hashes: all fifteen
rows must report `scram_passwords = 15`.

```sql
SELECT pg_catalog.count(*) FILTER (
    WHERE role.rolpassword LIKE 'SCRAM-SHA-256$%'
) AS scram_passwords
FROM pg_catalog.pg_authid AS role
WHERE role.rolname IN (
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
);
```

Run the enable manifest with the same independently reviewed target values. It
is the only manifest that changes request roles to `LOGIN`. It refuses to do so
unless every role has a SCRAM verifier, no sessions or prepared transactions
exist, role attributes and memberships are exact, no managed or public
parameter ACL remains, no request role owns an object, no request role can
connect to another database, and the exact schema, relation, function, and
default-privilege contract is intact.

```bash
STAGING_DATABASE=starring_staging
STAGING_DATABASE_HOST=replace_with_staging_database_host
STAGING_DATABASE_PORT=5432
STAGING_CLUSTER_ADMIN=replace_with_staging_cluster_admin
STAGING_SYSTEM_IDENTIFIER=replace_with_reviewed_staging_system_identifier
PGOPTIONS="-c starring.expected_staging_database=$STAGING_DATABASE -c starring.expected_staging_system_identifier=$STAGING_SYSTEM_IDENTIFIER" \
  psql --no-psqlrc --set ON_ERROR_STOP=1 --password \
    --host "$STAGING_DATABASE_HOST" --port "$STAGING_DATABASE_PORT" \
    --dbname "$STAGING_DATABASE" --username "$STAGING_CLUSTER_ADMIN" \
    --file ops/postgres/staging-api-role-enable.sql
unset STAGING_SYSTEM_IDENTIFIER STAGING_CLUSTER_ADMIN \
  STAGING_DATABASE_PORT STAGING_DATABASE_HOST STAGING_DATABASE
```

After enable succeeds, prove a wrong password fails and every request role is
denied on a different database before provisioning the fifteen distinct
database URLs through the prompt-only Keychain flow. Do not restore the API,
tunnel, or external ingress before those negative probes and aggregate API
readiness are green. A manifest failure or any unexpected capability keeps the
roles quarantined and staging ingress closed. This procedure remains a staging
rehearsal and does not make the current slice production-ready.

These staging manifests enforce the application capability surface they
enumerate; they are not a complete PostgreSQL language sandbox. A production
credential bootstrap must additionally inventory and close large-object,
language, type, foreign-data-wrapper, foreign-server, and tablespace ownership
and ACLs, then prove that inventory in an automated isolated-cluster gate.

## Starring API launch contract

The reviewed macOS LaunchAgent template is
`ops/macos/local.starring.api.staging.plist`. It is staging-only and contains
non-secret configuration and staging Keychain references. It does not contain
database URLs, OAuth secrets, bot tokens, key material, Cloudflare credentials,
or customer identifiers. There is intentionally no production LaunchAgent
template while Codex or any other untrusted workload shares this macOS login.

The template assumes a release binary installed at
`/Users/jungbogeon/.local/libexec/starring-api`, a user LaunchAgent running in
the same GUI login session as the Keychain items, and a log directory at
`/Users/jungbogeon/Library/Logs/starring-api`. Do not convert it to a system
LaunchDaemon without designing and validating a separate non-login secret
store.

### Exact non-secret environment

Every variable below is required. There are no implicit process-environment
defaults.

| Variable | Required contract | Template value |
| --- | --- | --- |
| `STARRING_API_BIND_PORT` | Integer 1024 through 65535; the process always binds IPv4 loopback regardless of host configuration | `18080` |
| `STARRING_API_PUBLIC_ORIGIN` | Canonical lowercase HTTPS domain origin with no explicit port, path, query, fragment, user information, or IP literal | `https://api.example.com` |
| `STARRING_API_OAUTH_RETURN_PATHS_JSON` | JSON array of 1 through 64 unique bounded local paths | `["/","/app"]` |
| `STARRING_API_OAUTH_DEFAULT_RETURN_PATH` | Exact member of the return-path array | `/app` |
| `STARRING_API_DATABASE_MAX_CONNECTIONS` | 1 through 4 per role; the complete A4/A5 template ceiling is 30 connections across 15 pools, while the mandatory core ceiling is 28 across 14 pools | `2` |
| `STARRING_API_DATABASE_ACQUIRE_TIMEOUT_MILLISECONDS` | 100 through 5000 milliseconds | `2000` |
| `STARRING_API_DATABASE_IDLE_TIMEOUT_SECONDS` | 30 through 600 seconds | `120` |
| `STARRING_API_DATABASE_MAX_LIFETIME_SECONDS` | 60 through 3600 seconds and strictly greater than idle timeout | `900` |
| `STARRING_API_DISCORD_APPLICATION_ID` | Actual nonzero Discord application ID; replace the template marker in the installed copy | required replacement |
| `STARRING_API_DISCORD_BOT_USER_ID` | Actual nonzero bot user ID for that application; replace the template marker in the installed copy | required replacement |
| `STARRING_API_DISCORD_REQUEST_TIMEOUT_MILLISECONDS` | 1 through 5000 milliseconds | `3000` |
| `STARRING_API_DISCORD_WRITE_AUTHORITY_LIFETIME_MILLISECONDS` | 1 through 5000 milliseconds | `3000` |
| `STARRING_API_DISCORD_READ_AUTHORITY_LIFETIME_MILLISECONDS` | 1 through 30000 milliseconds | `15000` |
| `STARRING_API_AUTHORING_WORKER_URL` | Canonical `http://127.0.0.1:<port>` origin with an explicit port from 1024 through 65535 and no credentials, path, query, or fragment | `http://127.0.0.1:18181` |

The OAuth callback registered with Discord must be exactly the configured
public origin plus `/oauth/discord/callback`. The public TLS endpoint and Host
must agree with this origin. An origin change is an OAuth configuration change,
not a DNS-only operation.

### Exact secret-reference environment

Secret-reference values use exactly `keychain:<service>:<account>` or
`env:<UPPERCASE_NAME>`. The macOS staging template uses Keychain. Its complete
A4/A5 authoring profile contains twenty pairwise-distinct resolved references:
fifteen database references, four core purpose references, and one authoring
worker bearer reference. The fourteen core database references and four core
purpose references are mandatory for general product composition. The writer
database and worker bearer references are accepted only together with the
canonical loopback worker URL; any incomplete, malformed, or aliased authoring
profile is excluded from authoring composition. The worker bearer reference is
Keychain-only even though the general reference grammar also supports
environment references. Raw worker tokens are forbidden.

| Variable | Capability | Template Keychain identity |
| --- | --- | --- |
| `STARRING_API_OAUTH_FLOW_WRITER_DATABASE_SECRET_REFERENCE` | OAuth flow create and consume | `database.oauth-flow-writer` |
| `STARRING_API_SESSION_ISSUER_DATABASE_SECRET_REFERENCE` | Session issue | `database.session-issuer` |
| `STARRING_API_SESSION_API_DATABASE_SECRET_REFERENCE` | Session read, touch, and logout | `database.session-api` |
| `STARRING_API_SECURITY_REVOKER_DATABASE_SECRET_REFERENCE` | Security revocation | `database.security-revoker` |
| `STARRING_API_INSTALLATION_AUTHORITY_DATABASE_SECRET_REFERENCE` | Installation-authority read | `database.installation-authority-reader` |
| `STARRING_API_AUTHORIZED_SNAPSHOT_DATABASE_SECRET_REFERENCE` | Authorized encrypted generation snapshot read | `database.authorized-snapshot-reader` |
| `STARRING_API_PROMOTION_EXECUTOR_DATABASE_SECRET_REFERENCE` | Promotion publication and link | `database.promotion-executor` |
| `STARRING_API_DECISION_READER_DATABASE_SECRET_REFERENCE` | Approval preview and product decision read | `database.decision-reader` |
| `STARRING_API_APPROVAL_EXECUTOR_DATABASE_SECRET_REFERENCE` | Approval execution | `database.approval-executor` |
| `STARRING_API_REJECTION_EXECUTOR_DATABASE_SECRET_REFERENCE` | Rejection execution | `database.rejection-executor` |
| `STARRING_API_APPLY_EXECUTOR_DATABASE_SECRET_REFERENCE` | Apply execution | `database.apply-executor` |
| `STARRING_API_CANCELLATION_EXECUTOR_DATABASE_SECRET_REFERENCE` | Lifecycle-cancellation execution | `database.cancellation-executor` |
| `STARRING_API_DEPLOYMENT_STATUS_DATABASE_SECRET_REFERENCE` | Deployment status V1 read | `database.deployment-status-reader` |
| `STARRING_API_OPERATIONAL_STATUS_DATABASE_SECRET_REFERENCE` | Operational deployment status V2 read | `database.operational-deployment-status-reader` |
| `STARRING_API_AUTHORING_SESSION_WRITER_DATABASE_SECRET_REFERENCE` | Encrypted authoring session load, replay check, and atomic generation commit | `database.authoring-session-writer` |
| `STARRING_API_AUTHORING_WORKER_TOKEN_SECRET_REFERENCE` | Private loopback Codex worker bearer authentication | `com.starring.llm-api-key/llm-api` |
| `STARRING_API_DISCORD_OAUTH_CLIENT_SECRET_REFERENCE` | Discord OAuth token exchange | `discord.oauth-client-secret` |
| `STARRING_API_DISCORD_BOT_TOKEN_REFERENCE` | Fresh Discord guild-authority queries | `discord.bot-token` |
| `STARRING_API_PRODUCT_ACTION_KEYRING_SECRET_REFERENCE` | Product action digest creation and verification | `keyring.product-action` |
| `STARRING_API_SNAPSHOT_ENVELOPE_KEYRING_SECRET_REFERENCE` | Authorized snapshot encryption and decryption | `keyring.snapshot-envelope` |

The database and core-purpose template Keychain service is
`starring-api.staging`; the pre-existing worker bearer uses service
`com.starring.llm-api-key` and account `llm-api`. Each database item contains
one complete PostgreSQL URL for its capability login. All fifteen URLs must
identify the same database but authenticate as fifteen distinct roles with no
role membership. The writer URL authenticates only as
`starring_authoring_session_writer`; do not copy it into any core account.
Local loopback or Unix-socket connections may disable TLS. A remote database
URL must use full certificate and hostname verification. PostgreSQL startup
`options` are rejected. Use a distinct random password for every login even
though secret-reference uniqueness is the enforced startup boundary.

The URL parser accepts only `postgres` or `postgresql`, an explicit lowercase
role matching `[a-z][a-z0-9_]{0,62}`, an explicit 24 through 512 character
password containing only `A-Z`, `a-z`, `0-9`, `_`, `-`, `.`, `~`, one lowercase
database identifier under the same rule, one explicit nonzero port, and one
required `sslmode`. Percent escapes, fragments, duplicate parameters, unknown
parameters, startup `options`, and omitted authority fields are rejected. The
only query parameters are `sslmode`, an absolute Unix-socket `host`, `port`,
and an absolute `sslrootcert`. Socket URLs use the literal authority
`localhost`, `sslmode=disable`, no root certificate, and an explicit query
port. Remote TCP uses `sslmode=verify-full`; loopback TCP may disable TLS. These
are non-secret shape examples and the password marker must be replaced:

```text
postgresql://starring_identity_oauth:REPLACE_WITH_32_RANDOM_URLSAFE_CHARS@127.0.0.1:5432/starring_staging?sslmode=disable
postgresql://starring_identity_oauth:REPLACE_WITH_32_RANDOM_URLSAFE_CHARS@localhost/starring_staging?host=/private/tmp&port=5432&sslmode=disable
postgresql://starring_identity_oauth:REPLACE_WITH_32_RANDOM_URLSAFE_CHARS@db.staging.example:5432/starring_staging?sslmode=verify-full&sslrootcert=/absolute/path/to/ca.pem
```

Startup rejects ambient `PGAPPNAME`, `PGDATABASE`, `PGHOST`, `PGHOSTADDR`,
`PGOPTIONS`, `PGPASSFILE`, `PGPASSWORD`, `PGPORT`, `PGSSLCERT`, `PGSSLKEY`,
`PGSSLMODE`, `PGSSLROOTCERT`, and `PGUSER`. Connection construction sets every
accepted field explicitly and deliberately bypasses `.pgpass`; do not depend
on an operator shell, service environment, or home-directory password file to
complete a URL.

OAuth client secret and bot token items contain their exact provider values.
Each keyring item contains one compact JSON object with version `1`, one active
key, and zero through seven retired keys. Every material value is canonical
Base64 for exactly 32 cryptographically random bytes. Key IDs are immutable and
unique inside a keyring. Product-action and snapshot-envelope key material must
also be different across purposes. The structural shape is:

```json
{"version":1,"active":{"id":"replace-active-id","material":"replace-with-canonical-base64-of-32-random-bytes"},"retired":[]}
```

The shown material is deliberately invalid placeholder text. Generate real
material outside the repository and never paste it into a command argument,
shell history, plist, log, issue, or operational evidence.

### Keychain provisioning and preflight

Use Keychain Access or the following prompt-only pattern from the service GUI
account. The final `-w` causes `/usr/bin/security` to prompt instead of placing
the value in the command line. Never use `-A` and never enable shell tracing.

```bash
SERVICE=starring-api.staging
for ACCOUNT in \
  database.oauth-flow-writer \
  database.session-issuer \
  database.session-api \
  database.security-revoker \
  database.installation-authority-reader \
  database.authorized-snapshot-reader \
  database.promotion-executor \
  database.decision-reader \
  database.approval-executor \
  database.rejection-executor \
  database.apply-executor \
  database.cancellation-executor \
  database.deployment-status-reader \
  database.operational-deployment-status-reader \
  database.authoring-session-writer \
  discord.oauth-client-secret \
  discord.bot-token \
  keyring.product-action \
  keyring.snapshot-envelope
do
  /usr/bin/security add-generic-password -U -s "$SERVICE" -a "$ACCOUNT" -w || exit 1
done
unset ACCOUNT SERVICE
```

The authoring worker bearer already belongs to the private loopback worker
boundary. Do not create, replace, print, or copy it as part of API database
provisioning. Prove only that the fixed Keychain identity exists, without
requesting its value:

```bash
/usr/bin/security find-generic-password \
  -s com.starring.llm-api-key \
  -a llm-api >/dev/null
```

Verify lookup access without printing values. A Keychain prompt, missing item,
three-second lookup timeout, excessive output, invalid UTF-8, malformed secret,
or locked Keychain is a startup failure.

```bash
SERVICE=starring-api.staging
for ACCOUNT in \
  database.oauth-flow-writer database.session-issuer database.session-api \
  database.security-revoker database.installation-authority-reader \
  database.authorized-snapshot-reader database.promotion-executor \
  database.decision-reader database.approval-executor \
  database.rejection-executor database.apply-executor \
  database.cancellation-executor \
  database.deployment-status-reader \
  database.operational-deployment-status-reader \
  database.authoring-session-writer \
  discord.oauth-client-secret discord.bot-token \
  keyring.product-action keyring.snapshot-envelope
do
  /usr/bin/security find-generic-password -s "$SERVICE" -a "$ACCOUNT" -w \
    >/dev/null || exit 1
done
unset ACCOUNT SERVICE
/usr/bin/security find-generic-password \
  -s com.starring.llm-api-key \
  -a llm-api >/dev/null
```

Before enabling ingress, log out and back in or reboot the staging host and
prove that launchd can resolve every item without an interactive Keychain
prompt. If any prompt appears, leave ingress disabled and correct the item's
access policy. The current adapter invokes the signed system
`/usr/bin/security` binary, so the macOS account itself is the process-isolation
boundary: any untrusted process running as that user is already in the secret
threat boundary. This home server also runs Codex under the same account, so it
is not a production secret boundary. Use only disposable staging credentials
here. A production cutover requires a dedicated OS account or an independently
isolated secret broker, with Codex and every other untrusted workload outside
that boundary. Do not work around a prompt with broad cross-user Keychain
access.

### Binary and LaunchAgent installation

Do not bootstrap the template until the release binary target exists, all
workspace and PostgreSQL gates are green, the installed plist replacements are
complete, and the staging limitation above is accepted.

```bash
(
  set -euo pipefail
  BUILD_ROOT=""
  cleanup_starring_api_build() {
    if test -n "$BUILD_ROOT"
    then
      rm -rf "$BUILD_ROOT"
    fi
  }
  trap cleanup_starring_api_build EXIT
  git fetch --prune origin
  test -z "$(git status --porcelain)"
  test -n "$STARRING_APPROVED_RELEASE_REVISION"
  printf '%s\n' "$STARRING_APPROVED_RELEASE_REVISION" \
    | /usr/bin/grep -Eq '^[0-9a-f]{40}$'
  APPROVED_SHA="$(git rev-parse --verify "${STARRING_APPROVED_RELEASE_REVISION}^{commit}")"
  test "$(git rev-parse HEAD)" = "$APPROVED_SHA"
  git merge-base --is-ancestor "$APPROVED_SHA" origin/main
  BUILD_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/starring-api-build.XXXXXX")"
  CARGO_TARGET_DIR="$BUILD_ROOT/target" \
    cargo build --locked --release -p starring-api
  SOURCE_BINARY="$BUILD_ROOT/target/release/starring-api"
  SOURCE_SHA256="$(/usr/bin/shasum -a 256 "$SOURCE_BINARY" | /usr/bin/awk '{print $1}')"
  mkdir -p "$HOME/.local/libexec" "$HOME/Library/LaunchAgents" \
    "$HOME/Library/Logs/starring-api"
  chmod 700 "$HOME/.local/libexec" "$HOME/Library/Logs/starring-api"
  install -m 500 "$SOURCE_BINARY" \
    "$HOME/.local/libexec/starring-api"
  INSTALLED_SHA256="$(/usr/bin/shasum -a 256 \
    "$HOME/.local/libexec/starring-api" | /usr/bin/awk '{print $1}')"
  test "$SOURCE_SHA256" = "$INSTALLED_SHA256"
  printf 'revision=%s\nbinary_sha256=%s\n' "$APPROVED_SHA" "$INSTALLED_SHA256"
  install -m 600 ops/macos/local.starring.api.staging.plist \
    "$HOME/Library/LaunchAgents/local.starring.api.staging.plist"
  plutil -lint "$HOME/Library/LaunchAgents/local.starring.api.staging.plist"
) || exit 1
unset STARRING_APPROVED_RELEASE_REVISION
```

Set `STARRING_APPROVED_RELEASE_REVISION` from the independently approved change
record, never by reading the target checkout immediately before installation.
The exact clean `HEAD` must equal that immutable 40-character revision and be
reachable from fetched `origin/main`. The fresh target directory prevents an
older local artifact from being mistaken for the build, and the recorded
source and installed SHA-256 values must match. The fail-fast subshell removes
its fresh build directory on success or failure and exits the operator shell
before any later launch command after a failed guard, build, install, digest,
or plist check. Preserve the printed revision and digest in the staging release
evidence, then clear the approved-revision environment variable after
installation.

Edit only the installed plist. Replace `https://api.example.com`,
`REPLACE_WITH_DISCORD_APPLICATION_ID`, and
`REPLACE_WITH_DISCORD_BOT_USER_ID`. Verify that its public origin, Discord
callback, tunnel hostname, and release evidence all agree. Leave every secret
as a Keychain reference.

```bash
INSTALLED="$HOME/Library/LaunchAgents/local.starring.api.staging.plist"
if /usr/bin/grep -Eq 'REPLACE_WITH_|api\.example\.com' "$INSTALLED"
then
  echo "starring-api plist still contains placeholders" >&2
  exit 1
fi
plutil -lint "$INSTALLED"
test -x "$HOME/.local/libexec/starring-api"
test "$(
  /usr/libexec/PlistBuddy \
    -c 'Print :EnvironmentVariables:STARRING_API_AUTHORING_SESSION_WRITER_DATABASE_SECRET_REFERENCE' \
    "$INSTALLED"
)" = 'keychain:starring-api.staging:database.authoring-session-writer'
test "$(
  /usr/libexec/PlistBuddy \
    -c 'Print :EnvironmentVariables:STARRING_API_AUTHORING_WORKER_URL' \
    "$INSTALLED"
)" = 'http://127.0.0.1:18181'
test "$(
  /usr/libexec/PlistBuddy \
    -c 'Print :EnvironmentVariables:STARRING_API_AUTHORING_WORKER_TOKEN_SECRET_REFERENCE' \
    "$INSTALLED"
)" = 'keychain:com.starring.llm-api-key:llm-api'
if /usr/libexec/PlistBuddy \
  -c 'Print :EnvironmentVariables:STARRING_API_AUTHORING_WORKER_TOKEN' \
  "$INSTALLED" >/dev/null 2>&1
then
  echo "starring-api plist contains a raw authoring worker token" >&2
  exit 1
fi
```

Keep public ingress disabled, then load the user LaunchAgent:

```bash
DOMAIN="gui/$(id -u)"
INSTALLED="$HOME/Library/LaunchAgents/local.starring.api.staging.plist"
launchctl bootstrap "$DOMAIN" "$INSTALLED"
launchctl print "$DOMAIN/local.starring.api.staging"
```

The plist restarts nonzero exits with a 30-second throttle. A clean exit is not
restarted automatically. Its 90-second launchd exit deadline covers the normal
15-second HTTP drain and 15-second concurrent database-pool close, plus bounded
startup cancellation while a registered signal waits for an in-progress
Keychain or aggregate-readiness phase to finish and release resources. The
service umask is `077`; the log directory and installed plist must remain
readable only by the service account.

### Startup and deep-readiness proof

Startup is deliberately fail-closed and ordered:

1. Parse every mandatory non-secret value and the eighteen unique core secret
   references. For the fixed A4/A5 authoring profile, also parse the writer
   database reference, canonical loopback worker URL, and Keychain-only worker
   bearer reference. A raw worker-token environment value, partial authoring
   tuple, malformed URL, or alias with any core or authoring purpose excludes
   the complete authoring tuple rather than exposing a partial dependency.
2. Resolve the eighteen core references and validate database URLs, OAuth
   secret, bot token, both keyring payloads, and cross-purpose key-material
   separation. A fully composed authoring profile resolves two more references,
   for exactly twenty resolved references in total. Writer-database or worker
   bearer resolution failure keeps authoring unavailable; it does not borrow a
   core credential or stop the independently ready core product surface.
3. Connect all fourteen bounded core pools. On partial core failure, begin
   closing every pool that connected and stop. Independently connect the
   fifteenth `starring_authoring_session_writer` pool only from its own URL.
4. Build the core facade and run aggregate core database capability readiness
   with a 45-second composition deadline. This verifies one database, fourteen
   distinct direct-login core roles, exact executable allowlists, relation and
   schema denial, absence of explicit parameter privileges and per-role
   database settings, installation authority, snapshot encryption-key
   coverage, action-key coverage, decision paths, and both deployment-status
   readers.
5. In parallel with core readiness, give authoring database readiness and the
   private worker contract preflight separate five-second deadlines. Authoring
   is composed only when the isolated writer proves its exact database identity,
   function allowlist, direct relation denial, key coverage, and shared logical
   database, and the worker proves its bounded contract. On authoring failure,
   close the writer pool and leave authoring routes fail-closed while preserving
   the core result.
6. Bind only `127.0.0.1:<configured-port>`.
7. While the listener is bound but the readiness gate is still closed, run the
   core facade readiness probe again with the server's 10-second startup
   deadline. False, panic, timeout, or shutdown returns a stable typed failure
   without a ready pulse.
8. Open general readiness only after the post-bind core probe succeeds. A
   single atomic lease owns general readiness for the lifetime of the server.
   General `/health/ready` is therefore not proof that authoring was composed.
   After optional composition finishes, the process emits exactly one closed
   startup line: `starring_api_authoring_status=ready` or
   `starring_api_authoring_status=unavailable`. A release that declares the
   A4/A5 product surface complete must retain the `ready` line from the current
   process start together with general readiness. The line contains no role,
   URL, token, database value, worker response, or error detail. It proves
   composition only; it does not satisfy the A6 live Luna,
   encrypted-generation, or promotion milestone.

The HTTP server defaults are 512 accepted connections, a ten-second HTTP/1
header deadline, 64 HTTP/1 headers, 64 KiB HTTP/1 buffer, 64 concurrent HTTP/2
streams per connection, 16 KiB HTTP/2 header list, 64 KiB HTTP/2 send buffer,
30-second idle HTTP/2 ping, ten-second ping acknowledgement deadline, and a
15-second graceful drain. These are code-owned validated production defaults,
not environment overrides.

The connection semaphore bounds memory and file descriptors; it is not a
same-host admission identity. A process that can run under the service account
can deliberately retain loopback HTTP/2 connections and exhaust availability.
Run no untrusted workload under that account, monitor listener and descriptor
occupancy, and treat same-user code execution as host compromise. If the host
later becomes multi-tenant, introduce an authenticated local proxy or
OS-enforced process boundary and re-run the availability threat model before
production use.

Verify the local listener and both health boundaries using the configured
public Host. Do not record response bodies.

```bash
PORT=18080
PUBLIC_HOST=api.example.com
/usr/sbin/lsof -nP -iTCP:"$PORT" -sTCP:LISTEN
/usr/bin/curl --fail --silent --show-error \
  --header "Host: $PUBLIC_HOST" \
  "http://127.0.0.1:$PORT/health/live" >/dev/null
/usr/bin/curl --fail --silent --show-error \
  --header "Host: $PUBLIC_HOST" \
  "http://127.0.0.1:$PORT/health/ready" >/dev/null
unset PORT PUBLIC_HOST
```

The listener must be exactly `127.0.0.1`, never `*`, `0.0.0.0`, a LAN address,
or a public address. Liveness proves only that the process event loop responds.
Readiness reads only the atomic server lease and never runs a database probe in
the request path. A single background supervisor runs aggregate fourteen-role
core readiness every 30 seconds with a ten-second deadline and no overlap. The
first error, timeout, or panic closes business admission, removes the listener,
drains accepted work, and exits with `server_runtime_readiness_failed`. The
maximum scheduled detection window is 40 seconds. `/health/ready` and every
non-health route return 503 `dependency_unavailable` while the lease is closed.

The background supervisor intentionally retains the core readiness contract;
it does not continuously probe the optional writer pool or private worker.
Authoring request failures remain scoped to the authoring boundary. Monitor
authoring saturation and dependency-unavailable outcomes separately, and treat
loss of writer or worker availability as authoring degradation rather than as
permission to substitute a core pool. Recomposition after a failed startup
authoring preflight requires a controlled process restart; the running facade
does not hot-add authoring dependencies.

### Authoring degraded operation

General `GET /health/ready` can remain `200` while authoring is unavailable.
The authoritative startup distinction is the current process's single redacted
line:

```text
starring_api_authoring_status=ready
starring_api_authoring_status=unavailable
```

When the line is `unavailable`, authentication, status, approval, Apply, and
other independently composed core routes may remain available, but authoring
turns fail closed with `503 dependency_unavailable`. An in-process authoring
capacity rejection returns `503 authoring_saturated` with the bounded
`Retry-After` configured by the HTTP boundary. A bounded dependency timeout is
`504 dependency_timeout`; a structurally invalid worker response is `502
upstream_invalid_response`. These public codes are redacted and retryability is
closed by code. Never surface a worker response, model transcript, database
error, Keychain result, or bearer credential to diagnose them.

For degradation:

1. Keep the core API running only while general readiness remains green.
2. Stop new authoring turns; preserve each caller's original idempotency key
   and expected generation.
3. Check the writer and worker by their independent least-privilege and
   loopback health procedures. Do not borrow a core database pool or bypass the
   worker contract.
4. Wait for active authoring work to settle, then perform a controlled API
   restart. Authoring dependencies are composed only at process startup.
5. Require general readiness and the new process's exact
   `starring_api_authoring_status=ready` line before reopening authoring.
6. On a lost turn response, retry the same idempotency key. Never create a new
   key until the exact session generation proves whether the prior commit won.

Repeated saturation is a capacity/SLO incident, not permission to increase
worker concurrency, queue depth, HTTP admission, database pool size, or timeout
without a measured cohort. If core readiness also fails, close public ingress
and follow the complete shutdown path instead of operating in degraded mode.

Before promoting a release candidate, rehearse the 14-role core probe and the
isolated fifteenth authoring pool under the expected concurrent request load and
database latency with the intended pool limit. Record core probe duration,
authoring preflight duration, worker contract latency, query volume, per-pool
occupancy, request latency, false readiness exits, restart time, and database
saturation. Confirm that every probe can acquire its required connection
without starving business work. Until that evidence defines an operational
margin and alert threshold, the template pool size, 30/10-second core schedule,
and five-second authoring startup deadlines are staging defaults rather than a
production SLO.

### Runtime recovery-required boundary

An API process or product status response must never reinterpret a runtime
`recovery_required` effect as retryable authoring, approval, or Apply work. The
affected route remains blocked while independently healthy routes retain only
their own authority. Operators inspect the redacted aggregate through
`ops/postgres/staging-runtime-interaction-effect-inspection.sql` and the runtime
runbook. That repeatable-read projection emits only block code, action kind,
count, and time bounds after verifying the exact 125-entry ledger and schema
manifest. A zero-row result proves only that snapshot; a nonzero result is not
permission to edit a receipt or effect table, replay a Discord mutation, or
delete a resource manually.

Recovery proceeds only through deterministic observation of the journaled
postimage or bounded compensation with the exact retained preimage. If the
runtime cannot establish that identity, keep the route closed and escalate the
stable block code. API rollback, API restart, or a new idempotency key cannot
clear this state.

Cloudflare Tunnel is a separate service and may be enabled only after local
readiness is green. Route it to the loopback address, keep its credentials in
its own secret store, and do not add Cloudflare credentials or forwarded-header
trust to the Starring API plist. A staging edge requires Cloudflare Access in
front of every routed product path and an edge rate rule for
`/oauth/discord/start` before the tunnel is enabled. Route only
`/oauth/discord/*`, `/v1/*`, and `/v2/*`; `/health/live` and `/health/ready`
remain loopback-only and must not have a public tunnel rule. Deep readiness is
database-intensive and is not a public probe. The exact edge threshold must be
derived from the disposable-user load rehearsal and remain below the bounded
database-pool capacity. Access and the edge rule are compensating staging
controls, not product authorization, and do not replace OAuth, session, CSRF,
tenant, installation, or fresh Discord authority checks. Public production
ingress remains prohibited until actor/session fairness controls complement the
process-wide OAuth-start fuse, identity retention runs through a monitored
scheduler, readiness load and restart SLOs are proven, the production runtime
worker exists, and the secret boundary is isolated from Codex.

The OAuth callback query carries short-lived `code` and `state` credentials.
Before enabling the tunnel, configure Cloudflare and every origin-side access
log to omit query strings for `/oauth/discord/callback` or redact those fields
before persistence. Restrict log access, define bounded rotation and deletion,
send one synthetic failed callback, and inspect every edge, tunnel, and origin
log sink to prove neither value was retained. A path-only request log is
sufficient; callback query material must never enter release evidence or an
incident ticket.

### Shutdown and stable-failure handling

A controlled SIGTERM or launchd bootout closes the readiness lease and listener
before draining active HTTP/1 and HTTP/2 work. HTTP drain is bounded to 15
seconds; connections still pending at the deadline are aborted and joined.
Only after the server returns should the process close all fourteen core
database pools and the optional fifteenth authoring pool concurrently, with a
separate 15-second deadline. A pool-close timeout is a stable redacted shutdown
failure, not permission to leave another instance running.

```bash
DOMAIN="gui/$(id -u)"
launchctl bootout "$DOMAIN/local.starring.api.staging"
```

Failure handling is intentionally closed:

- Invalid configuration or secret resolution exits before database composition
  or bind.
- Database connection, topology, ACL, function, key coverage, or deep-readiness
  drift keeps ingress closed and closes connected pools. Runtime deep-readiness
  loss is terminal on its first observed error, timeout, or panic; launchd may
  attempt a fresh composition only after its 30-second throttle.
- The first listener accept error immediately removes readiness. Accept retries
  use a cancellable one-second backoff; five consecutive errors terminate with
  a redacted stable failure.
- A competing server cannot claim or clear the active server's readiness gate.
- Repeated launchd failures indicate a stable fault. Boot out the job, preserve
  only redacted exit metadata, fix the cause, rerun preflight, and bootstrap it
  again. Do not let launchd retry indefinitely during database migration or
  credential repair.
- Never substitute owner, migrator, runtime, maintenance, or another capability
  credential to make readiness green.

Do not enable the public tunnel until local readiness, a staging OAuth flow, and
a disposable-guild authority check are green. B6 proved one staging Live route,
serving lease, and interaction path, but that evidence is not the final
disposable-guild Phase D certificate. Keep production ingress prohibited until
the exact merge candidate passes the release cohorts and merged-main CI.

## Preflight

1. Record the running application revision and migration version.
2. Take and verify a restorable PostgreSQL backup using the
   [backup, restore, and failure-drill contract](./2026-07-29-macos-starring-runtime-staging-operations.md#backup-restore-and-failure-drill-contract).
3. Stop new promotion, approval, rejection, and apply requests.
4. Drain legacy writers, including every old `interaction-smoke` process, and
   confirm no activation is `applying`.
   Before `202607240013_fence_runtime_execution_slot_writer_epoch.sql` and
   `202607240014_fence_runtime_execution_selector_slot_writer_epoch.sql`, set
   the exact runtime execution executor role to `NOLOGIN`, remove every
   membership into that role, drain every other client session from the target
   database, and resolve every prepared transaction in that database. Apply
   both migrations in the same stopped-maintenance window without reopening
   the executor between them. Keep runtime ingress isolated until migration,
   the matching runtime binary, and its readiness probe are green. Migrations
   013 and 014 reject a login-capable or inherited executor, any other client
   session, and any prepared transaction because legacy execution bodies do
   not share the complete physical slot fence and cannot participate in a
   rolling mixed-version cutover.
   After the migration and matching runtime binary are installed, keep ingress
   isolated, restore `LOGIN` only on the exact runtime execution role, and run
   one readiness probe through that role. If the probe fails, stop the new
   process, return the role to `NOLOGIN`, drain its session, and keep ingress
   closed. Start the runtime and reopen ingress only after that exact-role probe
   is green; never restore the legacy binary against migrations 013 or 014.
5. Confirm every product-authored promotion is provisioned into exactly one
   active tenant installation with the same tenant, guild, and RuleSet key.
6. Estimate table and index size and schedule a maintenance window for the
   table locks, synchronous index builds, artifact rewrite, authority-snapshot
   key-coverage scan, and runtime-attempt preflight in migrations 004, 006,
   007, 012, 013, `202607200003`, and `202607200006`.
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

SELECT
    (SELECT pg_catalog.count(*)
     FROM public.runtime_attestations) AS retained_attestations,
    (SELECT pg_catalog.count(*)
     FROM public.runtime_serving_leases) AS retained_serving_leases,
    (SELECT pg_catalog.count(*)
     FROM public.runtime_deployments AS deployment
     WHERE deployment.phase <> 'requested'
        OR deployment.revision <> 1
        OR deployment.controller_id IS NOT NULL
        OR deployment.controller_fencing_token IS NOT NULL
        OR deployment.controller_acquired_at IS NOT NULL
        OR deployment.controller_lease_expires_at IS NOT NULL
        OR deployment.last_fencing_token IS NOT NULL
        OR deployment.next_retry_at IS NOT NULL
        OR deployment.last_stable_error_code IS NOT NULL
        OR deployment.live_attestation_id IS NOT NULL
        OR deployment.live_at IS NOT NULL
        OR deployment.blocked_at IS NOT NULL
        OR deployment.superseded_at IS NOT NULL
        OR deployment.cancelled_at IS NOT NULL
        OR deployment.snapshot -> 'controller_lease'
            IS DISTINCT FROM 'null'::JSONB
        OR deployment.snapshot -> 'last_fencing_token'
            IS DISTINCT FROM 'null'::JSONB
        OR deployment.snapshot -> 'preflight' IS DISTINCT FROM 'null'::JSONB
        OR deployment.snapshot -> 'drain' IS DISTINCT FROM 'null'::JSONB
        OR deployment.snapshot -> 'activation' IS DISTINCT FROM 'null'::JSONB
        OR deployment.snapshot -> 'panel_certificate'
            IS DISTINCT FROM 'null'::JSONB
        OR deployment.snapshot -> 'gateway_ready'
            IS DISTINCT FROM 'null'::JSONB
        OR deployment.snapshot -> 'live' IS DISTINCT FROM 'null'::JSONB
        OR deployment.snapshot -> 'last_live_recovery'
            IS DISTINCT FROM 'null'::JSONB
        OR deployment.snapshot -> 'last_runtime_failure'
            IS DISTINCT FROM 'null'::JSONB) AS non_pristine_deployments;
```

Every control-plane failure count, `ruleset_artifact_shape_failures`, and every
returned shadow mismatch count must be zero. The three runtime-attempt counts
must also be zero before migration `202607200003`. Record the artifact row,
table-size, and largest-definition values for the migration rehearsal. A
nonzero failure count stops the cutover; do not weaken or skip the migration
constraints. Migration `202607200003` intentionally has no inference or
backfill path for retained runtime history. Preserve the database and design a
separate reviewed forward migration, or restore a verified pristine rehearsal
database; never delete production evidence merely to satisfy this preflight.

## Migration sequence

1. Keep API and runtime processes stopped.
2. Apply all pending migrations with the migrator credential.
3. Do not retry a failed migration blindly. Capture SQLSTATE and the stable
   constraint message, repair the preflight data through an audited operator
   path, then restart from a fresh transaction.
4. Run schema, function-signature, ownership, grant, default-privilege, RLS,
   and direct-DML denial probes.
5. Apply `ops/postgres/staging-api-role-bootstrap.sql` with the exact
   `ON_ERROR_STOP` and staging acknowledgement procedure above, assign all
   fifteen distinct passwords through prompt-only `\password`, then apply
   `ops/postgres/staging-api-role-enable.sql`. Complete the negative
   authentication probes before running aggregate product API readiness with
   fourteen distinct core direct-login pools and the isolated authoring-writer
   readiness probe against the same logical database.
6. Start only the API readiness process. It must verify product-action receipt
   key coverage, snapshot-envelope key coverage, every exact executable
   allowlist, and both deployment-status readers.
7. Start the monitored identity-retention scheduler separately. Do not start a
   runtime worker until its independent production contract exists.
8. Re-enable ingress only after every least-privilege probe is green.

Migration 004 deliberately takes strong locks and fails when legacy promotions
cannot be scoped to provisioned installations. Migrations 006 and 007 build
bounded retention indexes synchronously. Migration 007 is forward-only after
its first successful receipt purge because live replay receipts may no longer
exist; rollback then requires backup restore or a forward fix.

Migration `202607200003` takes access-exclusive locks on the three runtime
relations and has five-second lock and thirty-second statement bounds. It adds
durable convergence-attempt identity only to a pristine runtime history. The
preflight counts above are its cutover gate; a nonzero result requires a
separately reviewed forward migration rather than an inferred backfill.

Migration `202607200004` preserves the V1 deployment-status signature and role,
creates an owner-only one-read core plus the V2 identity and status projection,
and seals public routine execution and routine defaults. Reapply only the exact
V1 and V2 reader grants in the manifest below, then run both readiness probes.

Migration `202607200005` validates every historical rejection state, adds the
durable rejection reason and receipt evidence, and seals the rejection
executor behind its identity, key-coverage, and reject functions. It also
normalizes routine defaults, so apply the exact rejection grant set before
aggregate readiness.

Migration `202607200006` must run inside one transaction. It uses five-second
lock and thirty-second statement bounds, holds access-share locks on its eight
authority and snapshot relations, scans all generation encryption-key IDs, and
seals the two database-identity functions plus snapshot key coverage. Rehearse
the full generation scan at production-like scale. More than eight historical
snapshot keys, an uncovered key, a hostile schema grant, or an unrelated user
routine with `PUBLIC EXECUTE` fails the migration closed.

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
qualified relations after `PUBLIC` privileges are revoked. It must grant the
dedicated `starring_installation_authority_reader` role schema usage and only
the two exact versioned signatures without grant option:

```sql
GRANT USAGE ON SCHEMA public TO starring_owner;
GRANT USAGE ON SCHEMA public TO starring_installation_authority_reader;
GRANT EXECUTE ON FUNCTION
    public.starring_product_installation_authority_reader_database_identity_v1()
TO starring_installation_authority_reader;
GRANT EXECUTE ON FUNCTION
    public.starring_product_installation_authority_read_v1(TEXT, TEXT, BYTEA)
TO starring_installation_authority_reader;
```

For this slice, `starring_installation_authority_reader` must have no `SELECT`,
`INSERT`, `UPDATE`, `DELETE`, `TRUNCATE`, `REFERENCES`, or `TRIGGER` privilege on
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
and authorized-snapshot access require independent certification. Do not
compensate by granting the API role direct table access.

Migration 015 creates the independently scoped product-session authentication
boundary. Session-only reads, mutation reads, and touches use three separate
volatile, strict, parallel-unsafe security-definer functions fixed to
`search_path=pg_catalog`. The two reads lock the exact session and principal
rows. The mutation read exposes only a SHA-256 comparison tag bound to the
session digest and stored CSRF digest; neither read exposes the stored CSRF or
OAuth verifier digest. Touch uses the database clock and an exact observed-row
compare-and-set. It inherits the current session's exact idle window and rechecks
revocation, active expiry, the 30-minute global idle maximum, and a minimum
one-second touch interval. When configuration tightens the idle policy, an older
session with a longer issued window stops sliding and expires at its current
deadline. Immediate policy enforcement requires explicit session revocation and
reissuance through a separate management boundary.
Migration 015 requires the two identity relations to be ordinary non-RLS tables
under one owner, strips non-owner and hostile default function grants, transfers
all three functions to that owner, and revokes `PUBLIC` execution in one
migration transaction.

Grant the dedicated session API role only these exact signatures without grant
option:

```sql
GRANT EXECUTE ON FUNCTION
    public.starring_product_session_read_v1(BYTEA)
TO starring_identity_session;
GRANT EXECUTE ON FUNCTION
    public.starring_product_session_mutation_read_v1(BYTEA)
TO starring_identity_session;
GRANT EXECUTE ON FUNCTION
    public.starring_product_session_touch_v1(
        BYTEA,
        TIMESTAMPTZ,
        TIMESTAMPTZ,
        TIMESTAMPTZ,
        DOUBLE PRECISION
    )
TO starring_identity_session;
```

For this authentication slice, `starring_identity_session` must have no table
or column privilege on `product_principals` or `product_auth_sessions`. Retain
the same direct-login, role-attribute, role-membership, database, and schema
restrictions required by the installation-authority slice. Call
`PostgresAuthentication::verify_readiness` through a direct
`starring_identity_session` login before opening ingress. It verifies exact
function and relation metadata, the common non-login owner, ACLs, capabilities,
disabled RLS, and actual data-independent execution. The metadata phase is
bounded repeatable-read and read-only. The execution phase must be bounded
read-write because both read functions take `FOR SHARE` locks; its impossible
31-byte digest cannot select a session, the expected read counts are both zero,
the expected touch count is zero, and the transaction is rolled back. Do not
weaken this probe to read-only. This is a capability and function-shape probe,
not privileged-DDL attestation. Migration checksum verification, restricted
DDL credentials, and schema-change audit evidence remain separate cutover
requirements.

Migration 016 creates the authorized promotion snapshot read boundary. It binds
the authoring session, owner principal, opaque session digest, tenant, and
installation in one bounded read-committed, read-only transaction. The single
joined function statement is the atomic database snapshot; the prior timeout
configuration statement cannot pin an older view. Its materialized
database clock rejects disabled principals and malformed, revoked, future,
expired, or overlong product sessions before returning any row. The result is
limited to the encrypted generation envelope and the durable metadata required
for the existing Rust ownership, scope, fresh Discord authority, generation,
binding, policy, authenticated-encryption, restored-snapshot, and artifact
checks. It never returns stored CSRF or OAuth verifier digests, generation
summaries, writer request digests, or authority creator request digests.

Migration 016 requires all seven referenced identity, tenant, installation,
authoring-session, generation, and authority-version relations to be ordinary
non-RLS tables under one owner. It strips non-owner and hostile default
function grants, transfers the function to that owner, and revokes `PUBLIC`
execution in the same transaction. Migration `202607200006` adds the topology
and key-coverage functions. Grant the dedicated snapshot role only these exact
signatures without grant option:

```sql
GRANT USAGE ON SCHEMA public TO starring_authorized_snapshot_reader;
GRANT EXECUTE ON FUNCTION
    public.starring_product_authorized_snapshot_reader_database_identity_v1()
TO starring_authorized_snapshot_reader;
GRANT EXECUTE ON FUNCTION
    public.starring_product_authorized_snapshot_read_v1(
        TEXT,
        TEXT,
        BYTEA,
        TEXT,
        TEXT
    )
TO starring_authorized_snapshot_reader;
GRANT EXECUTE ON FUNCTION
    public.starring_product_authorized_snapshot_key_coverage_v1(TEXT[])
TO starring_authorized_snapshot_reader;
```

For this slice, `starring_authorized_snapshot_reader` must have no table or
column privilege on
`product_principals`, `product_auth_sessions`, `product_tenants`,
`automation_installations`, `authoring_sessions`,
`authoring_session_generations`, or
`automation_installation_authority_versions`. Call
`PostgresAuthorizedPromotionSnapshots::verify_readiness` through a direct
`starring_authorized_snapshot_reader` login before opening ingress. It verifies the exact function
result and execution contract, all seven relation owners and RLS flags, ACLs,
database and role capabilities, and an impossible-scope 31-byte-digest probe in
a bounded read-only transaction.

The snapshot function's read is the authorization linearization point. Changes
committed before it are observed. An immutable generation that was current at
that instant may still be promoted if a state change commits after the read and
before the later promotion write. Closing that interval requires one atomic
snapshot-validation and promotion-write transaction rather than row locks that
end before decryption. PostgreSQL also cannot independently verify fresh
Discord evidence because that evidence is an in-process capability rather than
a database-verifiable signature. Rust remains authoritative for Discord
permissions, evidence freshness, authority digest, decryption, and artifact
validation. A caller that compromises the API database login and possesses a
valid session digest can invoke this function directly, but receives only the
encrypted envelope and its bounded metadata, never plaintext or stored CSRF or
OAuth verifiers. The returned session digest is exactly the caller-supplied
digest and does not reveal an additional credential.

Migration 017 moves OAuth flow creation and consumption, session issuance,
logout, and security revocation behind independently scoped versioned database
capabilities. The OAuth writer receives only flow create and consume. The
issuer receives only session issue. The session API receives the three
authentication functions from migration 015 plus logout read and commit. The
security revoker receives only security revocation. The Rust adapter requires
four pools and routes each operation exclusively to its matching pool.

Migration 018 replaces the session-issue function so an uncertain successful
commit can be reconciled after the OAuth flow expires. It looks up the session
already bound to the locked flow before applying the current-time expiry gate.
Post-expiry `exact_replay` requires the identical session and CSRF digests,
canonical principal and requested lifetimes, an unrevoked and unrevised
session projection, valid principal data, and historical causality of
`flow.consumed_at <= session.authenticated_at < flow.expires_at`. If no session
exists, the database clock must still be strictly before flow expiry. This is
proof of an earlier commit, not authority for a new issuance.

Migration 017 requires `product_oauth_flows`, `product_principals`, and
`product_auth_sessions` to be ordinary non-RLS relations under one owner. The
three authentication functions from migration 015 must already have that same
owner. The migration creates one `product_control_plane_identity` singleton
with a non-secret random UUID and four role-specific topology functions. It
normalizes that relation to the common owner and removes its non-owner table and
column grants. In the same migration transaction, it revokes `PUBLIC` and every
named non-owner grant from the ten new topology and lifecycle functions, all
four identity transition trigger functions, and
`starring_purge_product_identity_v1`, then transfers those functions to the
common relation owner. A failure in relation, RLS, owner, or function
prerequisites rolls back the complete migration.

Use four direct-login roles with no role membership. The target role manifest
names them `starring_identity_oauth`, `starring_identity_issuer`,
`starring_identity_session`, and `starring_identity_security`. Each must have
only database `CONNECT`, schema `USAGE`, and its exact function set, without
grant option. Revoke any old migration-015 authentication grant from
`starring_api` before assigning that set to `starring_identity_session`; a
second named grantee causes readiness to fail.

```sql
REVOKE EXECUTE ON FUNCTION
    public.starring_product_session_read_v1(BYTEA)
FROM starring_api;
REVOKE EXECUTE ON FUNCTION
    public.starring_product_session_mutation_read_v1(BYTEA)
FROM starring_api;
REVOKE EXECUTE ON FUNCTION
    public.starring_product_session_touch_v1(
        BYTEA,
        TIMESTAMPTZ,
        TIMESTAMPTZ,
        TIMESTAMPTZ,
        DOUBLE PRECISION
    )
FROM starring_api;

GRANT USAGE ON SCHEMA public
TO starring_identity_oauth,
   starring_identity_issuer,
   starring_identity_session,
   starring_identity_security;

GRANT EXECUTE ON FUNCTION
    public.starring_product_oauth_database_identity_v1()
TO starring_identity_oauth;
GRANT EXECUTE ON FUNCTION
    public.starring_product_oauth_flow_create_v1(
        BYTEA,
        BYTEA,
        TEXT,
        TEXT,
        DOUBLE PRECISION
    )
TO starring_identity_oauth;
GRANT EXECUTE ON FUNCTION
    public.starring_product_oauth_flow_consume_v1(
        BYTEA,
        BYTEA,
        TEXT,
        TEXT[]
    )
TO starring_identity_oauth;

GRANT EXECUTE ON FUNCTION
    public.starring_product_session_issuer_database_identity_v1()
TO starring_identity_issuer;
GRANT EXECUTE ON FUNCTION
    public.starring_product_session_issue_v1(
        BYTEA,
        TEXT,
        TEXT,
        TIMESTAMPTZ,
        TEXT,
        TEXT,
        BYTEA,
        BYTEA,
        DOUBLE PRECISION,
        DOUBLE PRECISION
    )
TO starring_identity_issuer;

GRANT EXECUTE ON FUNCTION
    public.starring_product_session_api_database_identity_v1()
TO starring_identity_session;
GRANT EXECUTE ON FUNCTION
    public.starring_product_session_read_v1(BYTEA)
TO starring_identity_session;
GRANT EXECUTE ON FUNCTION
    public.starring_product_session_mutation_read_v1(BYTEA)
TO starring_identity_session;
GRANT EXECUTE ON FUNCTION
    public.starring_product_session_touch_v1(
        BYTEA,
        TIMESTAMPTZ,
        TIMESTAMPTZ,
        TIMESTAMPTZ,
        DOUBLE PRECISION
    )
TO starring_identity_session;
GRANT EXECUTE ON FUNCTION
    public.starring_product_session_logout_read_v1(BYTEA)
TO starring_identity_session;
GRANT EXECUTE ON FUNCTION
    public.starring_product_session_logout_commit_v1(
        BYTEA,
        BYTEA,
        TIMESTAMPTZ
    )
TO starring_identity_session;

GRANT EXECUTE ON FUNCTION
    public.starring_product_security_revoker_database_identity_v1()
TO starring_identity_security;
GRANT EXECUTE ON FUNCTION
    public.starring_product_session_security_revoke_v1(BYTEA)
TO starring_identity_security;
```

Grant `CONNECT` on the production database separately to those four exact role
names. They must have no direct table or column privilege on any of the four
identity relations, database `CREATE` or `TEMPORARY`, schema `CREATE`, owner
membership, other role membership, superuser, `CREATEDB`, `CREATEROLE`,
replication, or `BYPASSRLS`. They must not receive any other
`public.starring_*` function capability.

Before ingress, call `PostgresProductIdentityStore::verify_readiness`, not only
the four component probes. It verifies each function's exact signature, result,
language, volatility, strictness, parallel safety, row estimate,
security-definer flag, fixed search path, owner, and ACL; relation ownership,
shape, RLS, direct-privilege denial, and every table-level and column-level ACL
grantee; direct-login role capabilities; and rollback-only execution. The
aggregate check also requires four different role names with
`current_user = session_user`, one exact logical database UUID, and one exact
database name. A green component probe does not authorize ingress when the
aggregate probe is absent or failing.

An independent environment restored from a production backup inherits the
logical database UUID. Before that clone receives any service connection, the
migrator must assign it a new `pg_catalog.gen_random_uuid()` in
`product_control_plane_identity` and record the rotation. A failover member of
the same logical database retains the existing UUID. Never rotate only one
member of a replication topology.

Every OAuth flow, session-issue, logout, and security-revocation transaction in
the product-identity adapter is explicitly `READ COMMITTED, READ WRITE` and sets
transaction-local `statement_timeout`, `lock_timeout`, and
`idle_in_transaction_session_timeout` to the bounded authentication timeout.
Authentication read and touch transactions set the same three deadlines.
Readiness metadata and rollback-only execution probes are also bounded by all
three. Restrict connection counts, pool and request concurrency, and
transaction age, and alert on abnormal direct function calls. An actor holding
a valid session digest can still consume its granted function capacity and
create bounded row-lock pressure, although it cannot enumerate the identity
tables, choose a sub-second touch interval, enlarge the current idle window, or
extend beyond absolute session expiry.

If the first session-issue commit returns an uncertain outcome, the adapter
makes one immediate bounded reconciliation call with the same raw session and
CSRF credentials and all other immutable inputs unchanged. It must not generate
a new credential pair after uncertainty. Only a fully validated `issued` or
`exact_replay` result resolves the call. Any second transaction or commit
failure, domain rejection, collision, or malformed projection remains
`CommitIndeterminate`; stop the authentication response and preserve redacted
operational evidence for investigation.

PostgreSQL cannot prove that the Rust-only `VerifiedDiscordIdentityV1`
capability came from a valid Discord code exchange and identity lookup. The
four-role split limits a stolen database credential to one operation family;
it does not protect against compromise of a process that can access both the
issuer credential and a consumed-flow digest. A stronger future boundary is a
signed, flow-bound Discord verification receipt that PostgreSQL can verify
before issuing a session. Do not describe credential separation as
cryptographic identity attestation.

Migration 017 removes every existing named non-owner grant from
`starring_purge_product_identity_v1(INTEGER)` while normalizing its owner and
ACL. After the migration, explicitly regrant only that exact function to
`starring_maintenance`, without grant option, and rerun the maintenance probe
before restarting retention. Never grant retention execution to any of the
four request-serving identity roles.

```sql
GRANT EXECUTE ON FUNCTION
    public.starring_purge_product_identity_v1(INTEGER)
TO starring_maintenance;
```

Migration 018 likewise removes `PUBLIC` and every named non-owner grant from
`starring_product_session_issue_v1`, including hostile default-function grants,
and restores the migration-017 owner and function contract. Reapply only the
exact `starring_identity_issuer` grant shown above, then rerun issuer and
aggregate identity readiness before reopening ingress. Do not retain a second
issuer grantee as a rollout fallback.

Migration 017 deliberately does not revoke pre-existing grants on the three
legacy identity relations. Migration 021 removes the product-decision reader's
need for those grants, and migration 022 removes direct Apply artifact reads,
but other staged adapters still prevent a global relation-ACL seal. The
identity readiness ACL scan detects those grants,
including column-only grants belonging to an unrelated role, and remains red.
Keep ingress closed. After every remaining path is moved behind an exact
function, apply a separate sealing migration that revokes every non-owner table
and column grant, then require aggregate identity readiness to turn green. Do
not reclassify the red readiness result as a warning.

Migrations 014 through 022 and `202607200001` through `202607200006` established
the pre-A4 fourteen-role core baseline: installation-authority reads,
authentication, authorized-snapshot reads, promotion, decision reads, approval,
rejection, Apply, and both deployment status projections are scoped behind
exact functions and aggregate execute-only readiness. Migration
`202607300001` adds the separate authoring writer boundary without broadening
any of those core pools. Current A4/A5 composition may therefore own fifteen
pools, while the original fourteen-role statement remains historical evidence
of the core topology rather than the current complete inventory. Runtime
convergence mutation remains a separate worker capability and is not part of
the API process. Direct table grants are not a valid workaround for any
request-serving role.

### Trusted authoring writer and loopback worker isolation

Migration `202607300001_add_trusted_authoring_generation_writer.sql` adds the
trusted encrypted-generation writer contract. The
`starring_authoring_session_writer` direct-login role receives database
`CONNECT`, schema `USAGE`, and only these five function identities:

```text
public.starring_authoring_session_writer_database_identity_v1()
public.starring_authoring_session_writer_check_v1(text,text,text,text,bigint,text[],text[],text[],text[])
public.starring_authoring_session_writer_load_v1(text,text,text,text,bigint)
public.starring_authoring_session_writer_commit_v1(text,text,text,text,bigint,text[],text[],text[],text[],text,text,text,text,bigint,bytea,bytea,text,text,smallint,text,jsonb,text,bigint,text,jsonb,text,bigint,text,bytea,text,bigint)
public.starring_authoring_session_writer_key_coverage_v1(text[],text[],text[])
```

The role must have no direct relation or sequence privilege, no role
membership, no schema or database creation capability, no grant option, and no
executable user function outside that allowlist. Readiness is run through the
writer's own direct login. It proves the shared logical-database identity,
function ownership and ACLs, direct-relation denial, exact key-identity
coverage, and a data-independent execution contract. A core API role, `PUBLIC`,
runtime role, maintenance role, or legacy `starring_api` role must not execute
these functions.

The API encrypts authoring snapshots before the writer commit with the active
XChaCha20-Poly1305 envelope key and a fresh 24-byte nonce. The database receives
the ciphertext, authenticated metadata, bounded safe projection, and keyed
request evidence; the model worker receives neither the database credential,
snapshot keyring, Discord credential, product session credential, nor direct
Discord or deployment authority. The worker is an HTTP dependency reachable
only at the canonical loopback origin configured by
`STARRING_API_AUTHORING_WORKER_URL`, authenticated with the independently
stored Keychain bearer. Never place the bearer itself in the plist, process
arguments, logs, receipts, or this runbook.

A5 registers these authenticated authoring routes:

```text
POST /v1/installations/{installation_id}/authoring/sessions/{session_id}/turns
GET  /v1/installations/{installation_id}/authoring/sessions/{session_id}
```

They return the closed dependency-unavailable outcome when authoring
composition is absent. POST requires the product session, exact Host and
Origin, CSRF token, valid idempotency key, installation ownership, fresh
Discord authority, and bounded input. GET requires the product session, exact
Host, installation ownership, fresh read authority, and exact
session-owner/tenant/installation scope. Both expose only validated safe
projections with `Cache-Control: no-store`. The model can propose a candidate
but cannot approve, Apply, deploy, or call Discord. Writer readiness and route
availability prove A4/A5 composition only. The A6 milestone additionally
requires live one-shot and multi-turn Luna evidence, encrypted generation
inspection, exact PreviewReady promotion, and an explicit stop before approval
or Apply.

## Product decision capability boundaries

`PostgresProductDecisions` requires three pools named for decision reads,
approval execution, and apply execution. Query code uses only the reader pool,
approval and receipt-key coverage use only the approval pool, and apply uses
only the apply pool. Production composition must not clone one pool into all
three fields.

Migration 019 adds three logical-database topology functions and normalizes the
approval and keyring-coverage functions. It requires the 13 directly referenced
approval relations to be ordinary non-RLS tables under one existing owner and
requires the existing approval and coverage functions to have that same owner.
It removes `PUBLIC`, named-role, and grant-option execution from all five
functions. Environment-specific grants are not preserved.

Before applying migration 020, execute as the current `public` schema owner or
`SET ROLE` to that owner. Revoke `CREATE` from `PUBLIC` and every named grantee
other than the schema owner, then verify `pg_namespace.nspacl`. A separate
migrator `CREATE` grant is not accepted even when the migrator is operationally
trusted:

```sql
REVOKE CREATE ON SCHEMA public FROM PUBLIC;

SELECT privilege.grantee::REGROLE, privilege.privilege_type
FROM pg_catalog.pg_namespace AS namespace
CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
    namespace.nspacl,
    pg_catalog.acldefault('n', namespace.nspowner)
)) AS privilege
WHERE namespace.nspname = 'public'
  AND privilege.privilege_type = 'CREATE'
  AND privilege.grantee <> namespace.nspowner;
```

Revoke every row returned by that query before continuing. Migration 020 also
requires the schema owner itself to be the common product owner,
`pg_database_owner`, or the current database owner.

Migration 020 closes the currently enumerated user-trigger support boundary.
The six tables mutated by approval carry 19 user-defined triggers; one approval
INSERT or UPDATE executes the applicable subset rather than all 19. The shared
trigger graph can read three additional ruleset and runtime relations. Before
migration, all 16 relations, the 17 approval-specific trigger functions that
remain in the post-migration manifest, the shared
`starring_runtime_desired_target_digest_v1(JSONB, BIGINT)` helper, and the
existing Apply lock wrapper, lock core, and finalization functions must already
have the same reviewed owner, be ordinary functions, remain security definers,
and have exactly `search_path=pg_catalog`. This is a metadata prerequisite, not
Apply executor certification.

Migration 020 validates each trigger's relation, function, row/statement level,
event, timing, enabled state, constraint and parent relation binding,
deferrability, initially-deferred state, normalized `WHEN` predicate,
update-column vector, argument count and bytes, and old/new transition-table
bindings. It schema-qualifies the one legacy `authoring_promotions` reference,
replaces the globally shared immutable-row trigger binding on two approval
tables with an approval-only function, makes the resulting 18 trigger functions
internal security-definer capabilities fixed to `search_path=pg_catalog`,
normalizes the digest helper, and removes every non-owner execution grant from
those resulting 18 functions and the helper. Request roles never receive direct
execution on these internal functions. Existing Apply functions and the legacy
global `reject_immutable_product_row()` remain outside that revoke scope.

Migration 021 replaces the product-decision adapter's direct 11-relation query
with `starring_product_decision_read_v1`. Its manifest also includes the
topology identity relation, so all 12 reader relations and both reader functions
must have one reviewed non-RLS owner. The function accepts the exact promotion,
tenant, installation, guild, principal, acting Discord user, and opaque
32-byte session digest. Identity or target mismatches return zero rows; inactive
or revoked persisted state is still returned for Rust to classify under the
existing public contract. The 49-column projection remains subject to all Rust
domain, authority-history, payload, binding, and phase validation.

Migration 021 rejects every same-name overload before creation, fixes
`search_path=pg_catalog`, caps the result at two rows, and strips `PUBLIC`,
named-role, grant-option, and grants inherited from hostile defaults from both
current reader functions. It verifies their exact owner and catalog metadata.
It does not rewrite `pg_default_acl` and deliberately preserves transitional
relation ACLs. Audit and restrict default function privileges, then remove every
non-owner table and column grant on the 12-reader manifest before expecting
reader readiness to become green.

Migration 022 replaces the Apply adapter's direct artifact-table read with a
seven-input bounded target-artifact function and adds Apply-only keyring
coverage. It normalizes the existing lock and finalizer plus their complete
internal helper and 24-trigger graph. At migration 022, the Apply caller
manifest was exactly five functions over 18 direct and transitive ordinary
non-RLS relations. Later runtime-drain migrations extend the current manifest
to the seven functions and 25 relations granted below. The lock, pure Rust
preparation, artifact validation, and finalizer remain in one bounded
`SERIALIZABLE, READ WRITE` transaction.

Treat both 021 and 022 with their matching binaries as stopped-maintenance
rollouts. Migration 021 removes environment-specific reader grants, while 022
removes environment-specific Apply and internal-function grants. Either new
grant set makes the previous binary's exact executable contract red. Drain old
processes, apply each migration as the common owner, install the matching
binary and exact grants, then run component and aggregate probes. Do not infer
mixed-version compatibility from preserved transitional relation ACLs, and do
not reopen whole-product ingress after 022 alone.

The common owner must be a `NOLOGIN` role satisfying the same owner restrictions
as the identity boundary. The `public` schema must not grant `CREATE` to
`PUBLIC`, a request-serving role, or any other untrusted named principal. The
database owner is a trusted operational principal. A separate migration role
must `SET ROLE` to the schema owner for migration 020. Migrations 021 and 022
require `current_user` to equal the common object owner, and that owner must
have effective `CREATE` on `public`. Grant only the temporary membership needed
for that audited `SET ROLE` handoff, then revoke it before readiness. The
migrator must not retain its own schema `CREATE` ACL. Internal trigger functions
use only `pg_catalog` in their path and every application relation reference is
schema-qualified.

After migrations 019 through 022 and
`202607280005_cancel_runtime_product_drain_v2`, create or verify four distinct
direct-login roles with no membership. Replace `starring_production` below with
the reviewed production database identifier. Revoke PostgreSQL defaults and
any old database/schema privileges before granting only the staged manifest:

```sql
REVOKE CONNECT, TEMPORARY
ON DATABASE starring_production
FROM PUBLIC;

REVOKE ALL PRIVILEGES
ON DATABASE starring_production
FROM starring_decision_reader,
     starring_decision_approval,
     starring_decision_apply,
     starring_decision_cancellation;

REVOKE ALL PRIVILEGES
ON SCHEMA public
FROM PUBLIC,
     starring_decision_reader,
     starring_decision_approval,
     starring_decision_apply,
     starring_decision_cancellation;

GRANT CONNECT
ON DATABASE starring_production
TO starring_decision_reader,
   starring_decision_approval,
   starring_decision_apply,
   starring_decision_cancellation;

GRANT USAGE ON SCHEMA public TO starring_owner;
GRANT USAGE ON SCHEMA public
TO starring_decision_reader,
   starring_decision_approval,
   starring_decision_apply,
   starring_decision_cancellation;

GRANT EXECUTE ON FUNCTION
    public.starring_product_decision_reader_database_identity_v1()
TO starring_decision_reader;
GRANT EXECUTE ON FUNCTION
    public.starring_product_decision_read_v1(
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        BYTEA
    )
TO starring_decision_reader;

GRANT EXECUTE ON FUNCTION
    public.starring_product_approval_executor_database_identity_v1()
TO starring_decision_approval;
GRANT EXECUTE ON FUNCTION
    public.starring_product_approve_v1(
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        TEXT,
        TEXT,
        BYTEA,
        BYTEA,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        TEXT,
        TEXT,
        TIMESTAMPTZ,
        TIMESTAMPTZ,
        TEXT,
        BOOLEAN,
        TEXT,
        TEXT,
        TEXT[],
        TEXT[],
        TEXT[],
        TEXT,
        TEXT,
        TEXT,
        TEXT
    )
TO starring_decision_approval;
GRANT EXECUTE ON FUNCTION
    public.starring_product_approval_keyring_coverage_v1(TEXT[], TEXT[])
TO starring_decision_approval;

GRANT EXECUTE ON FUNCTION
    public.starring_product_apply_executor_database_identity_v1()
TO starring_decision_apply;
GRANT EXECUTE ON FUNCTION
    public.starring_product_apply_lock_v1(
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        TEXT,
        TEXT,
        BYTEA,
        BYTEA,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        TEXT,
        TEXT,
        TIMESTAMPTZ,
        TIMESTAMPTZ,
        TEXT,
        BOOLEAN,
        TEXT,
        TEXT,
        TEXT[],
        TEXT[],
        TEXT[],
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        TEXT
    )
TO starring_decision_apply;
GRANT EXECUTE ON FUNCTION
    public.starring_product_apply_target_artifact_v1(
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        BYTEA,
        TEXT,
        TEXT
    )
TO starring_decision_apply;
GRANT EXECUTE ON FUNCTION
    public.starring_product_apply_finalize_v1(
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        TEXT,
        TEXT,
        BYTEA,
        BYTEA,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        TEXT,
        TEXT,
        TIMESTAMPTZ,
        TIMESTAMPTZ,
        TEXT,
        BOOLEAN,
        TEXT,
        TEXT,
        TEXT[],
        TEXT[],
        TEXT[],
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        JSONB,
        TEXT,
        JSONB,
        JSONB,
        JSONB
    )
TO starring_decision_apply;
GRANT EXECUTE ON FUNCTION
    public.starring_product_apply_keyring_coverage_v1(TEXT[], TEXT[])
TO starring_decision_apply;
GRANT EXECUTE ON FUNCTION
    public.starring_product_apply_begin_runtime_drain_v2(
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        TEXT,
        TEXT,
        BYTEA,
        BYTEA,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        TEXT,
        TEXT,
        TIMESTAMPTZ,
        TIMESTAMPTZ,
        TEXT,
        BOOLEAN,
        TEXT,
        TEXT,
        TEXT[],
        TEXT[],
        TEXT[],
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        TEXT
    )
TO starring_decision_apply;
GRANT EXECUTE ON FUNCTION
    public.starring_product_apply_consume_runtime_drain_v2(
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        TEXT,
        TEXT,
        BYTEA,
        BYTEA,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        TEXT,
        TEXT,
        TIMESTAMPTZ,
        TIMESTAMPTZ,
        TEXT,
        BOOLEAN,
        TEXT,
        TEXT,
        TEXT[],
        TEXT[],
        TEXT[],
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        BYTEA,
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        TEXT,
        TEXT,
        BYTEA,
        TEXT,
        BYTEA,
        TEXT,
        TEXT,
        BYTEA
    )
TO starring_decision_apply;

GRANT EXECUTE ON FUNCTION
    public.starring_product_lifecycle_cancellation_executor_database_identity_v1()
TO starring_decision_cancellation;
GRANT EXECUTE ON FUNCTION
    public.starring_product_lifecycle_cancellation_keyring_coverage_v1(TEXT[], TEXT[])
TO starring_decision_cancellation;
GRANT EXECUTE ON FUNCTION
    public.starring_product_cancel_runtime_drain_v2(
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        TEXT,
        TEXT,
        BYTEA,
        BYTEA,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        TEXT,
        TEXT,
        TIMESTAMPTZ,
        TIMESTAMPTZ,
        TEXT,
        BOOLEAN,
        TEXT,
        TEXT,
        TEXT[],
        TEXT[],
        TEXT[],
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        TEXT,
        TEXT,
        BIGINT
    )
TO starring_decision_cancellation;
```

Inventory `pg_namespace.nspacl` after the revokes and remove `CREATE` from every
untrusted named grantee, not only these four request roles. The schema owner and
database owner are trusted operational principals. A separate migrator uses the
owner role without retaining its own `CREATE` ACL. Do not grant database
`CREATE` or `TEMPORARY`, schema `CREATE`, table access, column access, grant
option, owner membership, any other membership, or another
`public.starring_*` function.

The four grant sets above are complete component credentials for decision
reads, approval, Apply, and lifecycle cancellation. The mutation readiness paths
use their own coverage functions and never depend on the approval credential
having run first. Do not start the whole product service or open ingress from
this component manifest alone. Apply the remaining capability manifest below,
seal relation ACLs, and require aggregate whole-process readiness. Runtime
mutation remains a separate worker boundary.

`PostgresProductDecisions::verify_approval_executor_readiness` verifies the
enumerated approval function, owner, role, 16-relation, internal trigger,
keyring, and rollback-only execution contract. It compares the caller's
executable set against the exact approval allowlist for every public
security-definer routine and every `public.starring_*` routine. An unrelated
routine in that scope is a hard `ExcessCapability` failure.

`PostgresProductDecisions::verify_apply_executor_readiness` verifies the exact
seven-function Apply interface, 25-relation manifest, full helper and trigger
contract, dedicated keyring coverage, trusted topology, and rollback-only lock,
artifact, finalizer, drain-begin, and drain-consumption probes.
Lifecycle-cancellation readiness verifies its exact three-function interface,
21-relation manifest, terminal journal contract, dedicated keyring coverage,
and rollback-only cancellation probe.
`verify_product_decision_boundary_readiness` runs reader, approval, Apply, and
lifecycle-cancellation readiness and then requires one logical database
UUID/name with four distinct direct-login roles. The older
`verify_approval_boundary_readiness` remains a compatibility gate and is not an
ingress decision. Any legacy table or column grant on a protected component
manifest intentionally makes readiness red.

These components do not inspect relations outside their enumerated manifests,
views, sequences, routines outside `public`, ordinary non-`starring_*`
security-invoker helpers, or schema privileges outside `public`. Those remain
mandatory inputs to the final whole-process schema, object, and executable
manifest. Green product-decision components are never evidence that those wider
capabilities are absent.

## Promotion, rejection, and status capability grants

The remaining four request roles are
`starring_promotion_executor`, `starring_decision_rejection`,
`starring_deployment_status_reader`, and
`starring_operational_deployment_status_reader`. Apply only the exact function
identities below after migrations `202607200002`, `202607200004`, and
`202607200005`. The block fails if an expected function is missing. It does not
create roles or credentials.

```sql
GRANT USAGE ON SCHEMA public TO
    starring_promotion_executor,
    starring_decision_rejection,
    starring_deployment_status_reader,
    starring_operational_deployment_status_reader;

DO $grants$
DECLARE
    entry RECORD;
BEGIN
    FOR entry IN
        SELECT manifest.role_name, manifest.function_identity
        FROM (
            VALUES
                ('starring_promotion_executor', 'public.starring_product_promotion_executor_database_identity_v1()'),
                ('starring_promotion_executor', 'public.starring_product_promotion_replay_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,bigint,text,text[],text[],text[])'),
                ('starring_promotion_executor', 'public.starring_product_promotion_prepare_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bytea,text,bigint,bigint,text,text,text,text,jsonb,jsonb,text,text,text[],text[],text[],text,text,text,text)'),
                ('starring_promotion_executor', 'public.starring_product_promotion_publish_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text)'),
                ('starring_promotion_executor', 'public.starring_product_promotion_approval_environment_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text)'),
                ('starring_promotion_executor', 'public.starring_product_promotion_activation_link_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text,jsonb)'),
                ('starring_promotion_executor', 'public.starring_product_promotion_repair_link_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text,bytea,jsonb,text,text,text[],text[],text[],text,text,text,text)'),
                ('starring_promotion_executor', 'public.starring_product_promotion_keyring_coverage_v1(text[],text[])'),
                ('starring_decision_rejection', 'public.starring_product_rejection_executor_database_identity_v1()'),
                ('starring_decision_rejection', 'public.starring_product_rejection_keyring_coverage_v1(text[],text[])'),
                ('starring_decision_rejection', 'public.starring_product_reject_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text)'),
                ('starring_deployment_status_reader', 'public.starring_product_deployment_status_reader_database_identity_v1()'),
                ('starring_deployment_status_reader', 'public.starring_product_deployment_status_read_v1(text,text,text,text,text,text,text,text,bytea)'),
                ('starring_operational_deployment_status_reader', 'public.starring_product_deployment_status_reader_database_identity_v2()'),
                ('starring_operational_deployment_status_reader', 'public.starring_product_deployment_status_read_v2(text,text,text,text,text,text,text,text,bytea)')
        ) AS manifest(role_name, function_identity)
        ORDER BY manifest.role_name, manifest.function_identity
    LOOP
        IF pg_catalog.to_regprocedure(entry.function_identity) IS NULL THEN
            RAISE EXCEPTION 'starring API capability function is unavailable'
                USING ERRCODE = '55000';
        END IF;
        EXECUTE pg_catalog.format(
            'GRANT EXECUTE ON FUNCTION %s TO %I',
            entry.function_identity,
            entry.role_name
        );
    END LOOP;
END;
$grants$;
```

The complete fifteen direct-login roles are the four identity roles, the
installation-authority reader, the authorized-snapshot reader, the promotion
executor, the decision reader, approval executor, rejection executor, Apply
executor, lifecycle-cancellation executor, the two status readers, and the
isolated authoring-session writer. Every role needs database `CONNECT` and
schema `USAGE`; no role may have database `CREATE` or `TEMPORARY`, schema
`CREATE`, relation or sequence privilege, role membership, grant option, or an
executable user function outside its exact readiness allowlist. Revoke legacy
`starring_api` grants rather than keeping them during rollout. General service
readiness requires `PostgresProductApiReadiness::verify_readiness` to prove one
logical database and fourteen distinct core direct-login identities. Authoring
composition separately requires writer readiness to prove the same database
and the distinct fifteenth direct-login identity; failure leaves authoring
unavailable and must never cause credential reuse.

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

## API credential and snapshot-key rotation

All secrets are resolved at process startup. Updating a Keychain item does not
change the running process; every rotation requires a controlled API restart
and the complete startup and post-bind readiness sequence.

### Database capability credentials

Rotate one capability login at a time. Preserve the exact role's grants and
direct-login restrictions, change only that role's credential, update only its
matching Keychain account, then restart the single API process. The service is
not eligible for ingress until aggregate readiness again proves one logical
database and fourteen distinct core roles, and an authoring-enabled release is
not eligible until the isolated fifteenth writer and loopback worker preflight
also succeed. Never copy one database URL into a second account as a temporary
fallback. Revoke the old credential only after the new process has passed local
readiness and the old process has exited.

If the database is remote, rotation evidence must also prove `verify-full`
certificate and hostname validation. Do not add PostgreSQL startup options or
weaken transport validation to recover from a rotation error.

### Discord OAuth client secret and bot token

Coordinate OAuth client-secret rotation with the exact configured callback.
Stop new OAuth starts, allow or invalidate the short in-flight flow window,
replace the `discord.oauth-client-secret` Keychain item, and restart. Run a
staging OAuth start and callback without retaining authorization codes, OAuth
tokens, cookies, state, or nonce values.

Rotate the Discord bot token by replacing only `discord.bot-token`, restarting,
and proving fresh manager authority against a disposable staging guild. A
failed Discord query keeps protected product operations unavailable; it is not
a reason to extend authority evidence lifetimes. Revoke the superseded provider
credential after the new process is green.

### Snapshot-envelope keyring

Snapshot-envelope rotation differs from ordinary credential rotation because
persisted encrypted generations retain their encryption key ID.

1. Generate a new random 32-byte key with a new immutable ID.
2. Replace the Keychain payload with the new key active and every still-needed
   old key retired.
3. Restart the API and require aggregate readiness to pass snapshot key-ID
   coverage for all retained encrypted generations.
4. Keep the old key while any retained generation references it. There is no
   authorization to delete rows or invent a new envelope merely to make
   coverage green.
5. Remove an old key only after an accepted bounded re-encryption or retention
   path proves no persisted envelope references it, then restart and rerun
   readiness.

The keyring holds at most eight total keys. Stop rotation before the limit is
reached and design the missing re-encryption or retention operation; never drop
an old key and accept unreadable snapshots. Snapshot material must never equal
any product-action key material.

### Rotation rollback

Keep the prior installed binary, non-secret plist, and still-valid old key
material through the rollout soak. If a new credential or keyring fails before
provider revocation or old-key destruction, boot out the process, restore the
previous Keychain payload using a prompt-only path, restore the previous binary
if necessary, and rerun every readiness gate. If the old provider credential
was revoked or old encryption material was destroyed, do not fabricate a
rollback; use a forward credential fix or a verified backup and documented
recovery procedure.

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
- For an API-only rollback, disable the public tunnel first, boot out
  `local.starring.api.staging`, restore the previously verified binary and
  installed non-secret plist, restore only still-valid Keychain entries,
  bootstrap the job, and require local liveness plus deep readiness before
  reopening staging ingress.
- A prior API binary must not be started against a schema or function manifest
  it does not recognize. When migrations are forward-only or receipt retention
  has run, use a forward fix or verified database restore rather than forcing
  an old binary through red capability probes.
- API rollback does not rewind a deployment, route, receipt, effect journal, or
  Discord resource. A compatible runtime may keep an exact deployment Live
  while the API is stopped. After any API rollback, keep public ingress closed
  until the API reports deep readiness and the exact product deployment status
  independently proves its current runtime state. Never infer Live from an
  Applied pointer or an API process restart.

## Evidence to retain

- application and migration revisions
- aggregate preflight counts
- migration duration and lock-wait metrics
- role capability-probe results
- product-identity aggregate and component readiness outcomes,
  authorized-snapshot readiness outcomes, function identities, and role names
  only
- aggregate fourteen-role core readiness, isolated fifteenth-writer readiness,
  the twenty-reference cardinality result, and the loopback worker contract
  preflight outcome; retain only reference identities, the non-secret loopback
  origin, bounded contract metadata, and stable redacted classifications
- retention deleted counts and backlog flags
- keyring coverage outcome and key IDs only
- launchd label, installed binary digest, non-secret plist digest, local
  listener address, liveness/readiness status codes, and stable redacted exit
  classification
- backup and restore-drill identifiers
- redacted duplicate-receipt counter deltas paired with durable final-state and
  external-resource evidence
- redacted recovery block groups and the validated migration-ledger identity
- D2 sealed checkpoint kinds, canonical resource-inventory digest, and
  Created-to-Deleted aggregate counts for manifest-owned resources only

Do not retain credentials, raw OAuth state, cookies, session or CSRF digests,
derived comparison tags, raw idempotency keys, RuleSet JSON, or user message
bodies in operational evidence.
