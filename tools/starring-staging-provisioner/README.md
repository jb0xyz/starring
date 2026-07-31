# Starring staging provisioner

This one-shot macOS staging tool runs after the dedicated PostgreSQL 16
database bootstrap and before the API and runtime role-enable manifests. It is
intentionally limited to the fixed `starring_runtime_staging` database,
`starring_cluster_admin` peer identity, the private
`/private/tmp/starring-bootstrap` Unix socket directory, and port 5432.

The provisioning mode verifies the independent system identifier and exact v2
containment acknowledgement, the temporary peer HBA rule, an idle target, the
passwordless `starring_owner`, and twenty passwordless `NOLOGIN`
application roles. It requires the three existing Discord Keychain items to be
readable and leaves them unchanged.

It generates twenty-one distinct 32-byte passwords, two independent 32-byte API
keyrings, one dedicated 32-byte runtime interaction-token envelope keyring, and
independent PostgreSQL SCRAM-SHA-256 verifiers. Plaintext
passwords are hex-encoded in memory and passed only to the interactive macOS
`security` process over stdin, never through an argument or environment
variable, and stored as fixed Keychain database URLs. Only verifier strings
are sent to PostgreSQL, in one transaction. If a failure occurs before the
database commit, previous managed Keychain values are restored. A
commit-indeterminate error preserves the newly written Keychain values for
operator reconciliation.

```zsh
env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin \
  "$HOME/.local/libexec/starring-staging-provisioner" \
  "$STAGING_SYSTEM_IDENTIFIER" \
  "starring-runtime-dedicated-staging-cluster-v2:$STAGING_SYSTEM_IDENTIFIER:starring_runtime_staging:cluster-wide-public-acl-reset:bidirectional-runtime-membership-revocation"
```

After the reviewed API and runtime enable manifests install `LOGIN`, install
and reload the final integrated HBA and run:

```zsh
env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin \
  "$HOME/.local/libexec/starring-staging-provisioner" \
  --verify-final \
  "$STAGING_SYSTEM_IDENTIFIER" \
  "starring-runtime-dedicated-staging-cluster-v2:$STAGING_SYSTEM_IDENTIFIER:starring_runtime_staging:cluster-wide-public-acl-reset:bidirectional-runtime-membership-revocation"
```

The final verifier reads the fixed administrator and twenty application
database URLs plus all three keyrings from Keychain without printing them. It
requires strict version-1 keyring payloads with distinct active IDs and
materials, validates the runtime keyring's active key and at most seven retired
keys, and proves direct non-TLS IPv4 loopback connections, exact database,
schema, role identities, and the fifteen-line final HBA contract. Physical
replication rejection remains a separate external negative probe because this
tool does not claim to model a replication-protocol startup.

An already provisioned nineteen-role staging cluster must not rerun the
one-shot mode. After the trusted authoring-writer migration is applied and the
reviewed final HBA containing `starring_authoring_session_writer` is installed
and reloaded, keep the API stopped and provision only the new writer:

```zsh
env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin \
  "$HOME/.local/libexec/starring-staging-provisioner" \
  --provision-authoring-writer \
  "$STAGING_SYSTEM_IDENTIFIER" \
  "starring-runtime-dedicated-staging-cluster-v2:$STAGING_SYSTEM_IDENTIFIER:starring_runtime_staging:cluster-wide-public-acl-reset:bidirectional-runtime-membership-revocation"
```

This mode reads the existing administrator URL from Keychain, validates the
final cluster and HBA, requires the exact writer migration and five-function
capability set, and creates only
`starring_authoring_session_writer` plus
`starring-api.staging/database.authoring-session-writer`. In the same
serializable transaction it performs the one required existing capability
cutover: `starring_authorized_snapshot_reader` loses v1 snapshot execute and
gains v2 snapshot execute. No other existing role, credential, keyring, or ACL
is changed. A second identical invocation returns
`authoring_writer=exact_replay` without rotating anything. Legacy writer state,
cutover snapshot state, asymmetric state, mixed v1/v2 access, and excess ACLs
are classified explicitly and fail closed unless they form one exact fresh or
replay state.

An already-live staging cluster that predates the runtime interaction receipt
keyring must not rerun the one-shot mode. Keep the API and runtime stopped and
run the dedicated incremental mode with an empty `PG*` environment:

```zsh
env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin \
  "$HOME/.local/libexec/starring-staging-provisioner" \
  --provision-interaction-token-keyring \
  "$STAGING_SYSTEM_IDENTIFIER" \
  "starring-runtime-dedicated-staging-cluster-v2:$STAGING_SYSTEM_IDENTIFIER:starring_runtime_staging:cluster-wide-public-acl-reset:bidirectional-runtime-membership-revocation"
```

This mode performs no PostgreSQL connection or mutation. It requires both
existing API keyrings to be readable and semantically valid, then either
creates only
`starring.runtime.staging/interaction.token-envelope-keyring` with independent
32-byte material or returns exact replay for a valid existing item. An invalid
API or runtime keyring fails closed. Successful output contains only
`outcome=created|exact_replay` and the active key ID. It never rotates an
existing item and never emits key material or a material hash.

This is a disposable staging same-login-boundary tool. It does not create a
production secret-isolation boundary: every process running under the same
macOS login remains inside the Keychain threat boundary.
