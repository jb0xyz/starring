# Starring staging database bootstrap

This tool creates the fixed `starring_owner` role and
`starring_runtime_staging` database, assigns the `public` schema directly to
the owner, applies the repository SQLx migration ledger under
`SET ROLE starring_owner`, and verifies the exact migration versions and
checksums, all 171 user-schema relations, all 102 API and runtime capability
function owners, and zero inbound or outbound owner memberships.

The ordinary mode accepts the cluster-administrator URL only from a hidden
`/dev/tty` prompt:

```zsh
env -i PATH="$PATH" starring-db-bootstrap \
  "$STAGING_SYSTEM_IDENTIFIER" \
  "starring-runtime-dedicated-staging-cluster-v2:$STAGING_SYSTEM_IDENTIFIER:starring_runtime_staging:cluster-wide-public-acl-reset:bidirectional-runtime-membership-revocation"
```

The fresh-cluster mode has no secret input:

```zsh
env -i PATH="$PATH" starring-db-bootstrap --peer-bootstrap \
  "$STAGING_SYSTEM_IDENTIFIER" \
  "starring-runtime-dedicated-staging-cluster-v2:$STAGING_SYSTEM_IDENTIFIER:starring_runtime_staging:cluster-wide-public-acl-reset:bidirectional-runtime-membership-revocation"
```

It is fixed to the dedicated Unix socket directory
`/private/tmp/starring-bootstrap`, port `5432`, database `postgres`, and role
`starring_cluster_admin`. Before using it, the temporary HBA must make the
first matching rule for both `postgres` and `starring_runtime_staging` an exact
peer rule using the map `starring_bootstrap`. The matching ident map binds the
reviewed bootstrap operating-system user to `starring_cluster_admin`.

Fresh-cluster mode requires the administrator password to be absent before any
administrator mutation. It then fixes that role to `LOGIN SUPERUSER
NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS`, connection
limit two, infinite validity, no password, no role settings, and no inbound or
outbound memberships. It verifies the complete seven-rule HBA and the sole
ident mapping before normalization. Ordinary URL mode does not alter the
administrator password or attributes.

The system identifier must come from the independently reviewed infrastructure
inventory. The tool compares it with `pg_control_system()` before creating the
owner or database. The acknowledgement is non-secret but must exactly repeat
that identifier and the fixed database and containment contract.

The temporary peer rule is bootstrap-only. After a successful run, atomically
replace it with the reviewed final HBA, reload PostgreSQL, verify
`pg_hba_file_rules`, and prove `starring-db-bootstrap --peer-bootstrap` can no
longer connect. The final HBA keeps only the reviewed loopback SCRAM path for
`starring_cluster_admin` and rejects every local-socket path.

Every mode rejects inherited `PG*` environment variables. The tool never
creates a role password, reads Keychain, accepts a password through command
arguments, or creates a permanent migrator or peer HBA rule.
