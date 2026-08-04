# Staging authority operator

This one-purpose operator advances the single integrated-staging installation
from authority revision 1 to revision 2. It preserves the revision-1 policy,
approval count, and activation TTL while adding exactly one reviewed Discord
text-channel binding:

```text
community_hub -> <discord_channel_id>
```

The operator reads the cluster-administrator URL only from the macOS Keychain
item:

```text
service: starring.postgres.staging
account: database.cluster-admin
```

It rejects PostgreSQL environment variables and accepts no secret argument.
The command requires the nonsecret cluster system identifier, installation ID,
reviewed Discord channel ID, and an acknowledgement that binds all three:

```sh
cargo run --locked -p starring-staging-authority-operator -- \
  '<system_identifier>' \
  '<installation_id>' \
  '<discord_channel_id>' \
  'starring-staging-authority-advance-v1:<system_identifier>:<installation_id>:1:2:community_hub:<discord_channel_id>:reviewed-discord-text-channel'
```

Before mutation it requires the exact staging database, loopback connection,
cluster identity, cluster-administrator role, owner role, migration, immutable
authority triggers, one active installation, empty revision-1 bindings, an
active creating principal, and no existing authoring, promotion, activation,
deployment, immutable RuleSet artifact, or active RuleSet pointer state for
that installation's exact guild and RuleSet slot.

The revision insert and installation-head compare-and-set run in one
serializable transaction under `starring_owner`. A fresh connection then reads
the committed state, compares every revision-1 field with the pre-mutation
snapshot, decodes the revision-2 bindings, recomputes the resource fingerprint,
recomputes both domain-separated authority digests, and verifies the revision-2
head.

Successful results are:

```text
advanced
exact_replay
recovered_committed
```

An exact invocation after a completed advance returns `exact_replay`. A
different channel or any different revision-2 content returns
`authority_input_conflicts` without mutation.
