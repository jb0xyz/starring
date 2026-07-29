# Starring staging provisioner

This one-shot macOS staging tool runs after the dedicated PostgreSQL 16
database bootstrap and before the API and runtime role-enable manifests. It is
intentionally limited to the fixed `starring_runtime_staging` database,
`starring_cluster_admin` peer identity, the private
`/private/tmp/starring-bootstrap` Unix socket directory, and port 5432.

The provisioning mode verifies the independent system identifier and exact v2
containment acknowledgement, the temporary peer HBA rule, an idle target, the
passwordless `starring_owner`, and nineteen passwordless `NOLOGIN`
application roles. It requires the three existing Discord Keychain items to be
readable and leaves them unchanged.

It generates twenty distinct 32-byte passwords, two independent 32-byte API
keyrings, and independent PostgreSQL SCRAM-SHA-256 verifiers. Plaintext
passwords are passed only to the macOS `security` process over stdin and stored
as fixed Keychain database URLs. Only verifier strings are sent to PostgreSQL,
in one transaction. If a failure occurs before the database commit, previous
managed Keychain values are restored. A commit-indeterminate error preserves
the newly written Keychain values for operator reconciliation.

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

The final verifier reads the fixed administrator and nineteen application
database URLs plus both API keyrings from Keychain without printing them. It
requires strict version-1 keyring payloads with distinct active IDs and
materials, direct non-TLS IPv4 loopback connections, exact database, schema,
and role identities, and the fifteen-line final HBA contract. Physical
replication rejection remains a separate external negative probe because this
tool does not claim to model a replication-protocol startup.

This is a disposable staging same-login-boundary tool. It does not create a
production secret-isolation boundary: every process running under the same
macOS login remains inside the Keychain threat boundary.
