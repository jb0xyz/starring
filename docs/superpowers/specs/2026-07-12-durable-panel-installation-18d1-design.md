# Phase 18d-1: RuleSet-driven Durable Panel Installation — Design

## Goal

Make the RuleSet's declared panels the source of truth for installation, and
reconcile them against actual Discord state on every boot: install a declared panel
once, don't re-post it on restart, edit it when its presentation changes, and
re-post it if the message was deleted. Remove the tool's hardcoded panel install.

## Context

The tool currently calls a hardcoded `install_panel("Study room panel", ...)`
unconditionally at boot, so every restart re-posts the entry panel (the 17e
duplicate). This is a correctness problem, not cosmetic: duplicate create/join
buttons mean users can't tell which is current, stale `custom_id`s stay clickable,
and the UI can diverge from the active RuleSet. The RuleSet already *declares*
`panels: [study_panel]`, but nothing installs from that declaration.

18d-1 closes this: installation becomes RuleSet-driven and durable, tracked in a new
persistent store. This is the last piece of "the bot is fully DB-driven."

Explicitly OUT of scope (separate phases): instance-internal `ActionSpec::PostPanel`
panels (18d-2 attach-after-register), orphan cleanup / message deletion, RouteId /
custom_id compaction (18d-3), and content-drift detection (re-reading a message's
body to compare — bot messages aren't user-editable, so unnecessary for the first cut).

## Global Constraints

- No code comments anywhere (`//`, `///`, `//!`).
- New crate pair, mirroring instance/ruleset: `automation-panel-installation` (pure
  domain + reconcile) and `automation-panel-installation-postgres` (Postgres store).
- `automation-panel-installation` forbidden deps: `sqlx`, `twilight-*`, HTTP impls,
  and any dependency on `automation-ruleset-readiness` / `RuntimeRuleSet` (the edge
  passes extracted values). Reverse deps forbidden: `automation-state` / `-ruleset` /
  `-readiness` must not depend on `automation-panel-installation`.
- Fail-closed and honest guarantees: a normally-completed install is not re-posted;
  a crash between Discord post and DB commit may duplicate (future reconciliation);
  transient Discord errors never trigger a re-post. This is **at-least-once +
  durable dedupe after commit, NOT exactly-once**. First cut assumes a **single
  installing process** (two concurrent boots may both post; multi-process needs a
  future advisory lock / claim).
- Gates: `$HOME/.cargo/bin/cargo test`, `clippy --all-targets -- -D warnings`,
  `fmt --check`. Postgres tests `#[ignore]`.

## Architecture

```
automation-panel-installation            pure: domain + store trait + installer trait + reconcile
├─ automation-state          (PanelSpec, ButtonSpec, ButtonRoute)
├─ automation-ruleset        (RuleSetVersionId — the installed version type)
├─ resource-resolution       (ResourceBindingMap — channel resolution)
├─ desired-state             (ResourceKey — PanelSpec.channel)
├─ discord-model             (GuildId, ChannelId, MessageId)
├─ serde / serde_json / sha2 (spec_hash)

automation-panel-installation-postgres   PostgresPanelInstallationStore + migration
└─ automation-panel-installation

automation-runtime                        TwilightPanelInstaller + PANEL_RENDER_REVISION
└─ automation-panel-installation

interaction-smoke tool                    assembles; no install rules of its own
├─ automation-panel-installation-postgres
└─ automation-runtime
```

`automation-panel-installation` depends on `automation-ruleset` only for the
`RuleSetVersionId` type of `installed_version` (a one-type addition to the domain-dep
list; `automation-ruleset` does not depend back, so no cycle).

## Domain types (`automation-panel-installation`)

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelInstallationKey {
    pub guild_id: GuildId,
    pub ruleset_key: RuleSetKey,
    pub panel_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelInstallation {
    pub guild_id: GuildId,
    pub ruleset_key: RuleSetKey,
    pub panel_key: String,
    pub installed_version: RuleSetVersionId,
    pub channel_id: ChannelId,
    pub message_id: MessageId,
    pub spec_hash: String,
}
```

Logical identity is `(guild, ruleset_key, panel_key)` — **version is not in the key**.
The same logical panel is one row across versions; only `installed_version` advances
(activation) or rolls back.

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelInstallationStoreError {
    Backend(String),
}

#[allow(async_fn_in_trait)]
pub trait PanelInstallationStore {
    async fn get(&self, key: &PanelInstallationKey)
        -> Result<Option<PanelInstallation>, PanelInstallationStoreError>;
    async fn upsert(&self, installation: PanelInstallation)
        -> Result<(), PanelInstallationStoreError>;
}
```

Plus `InMemoryPanelInstallationStore` (BTreeMap keyed by the logical key).

## Installer seam (edge-injected)

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelPresence {
    Present,
    Gone,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelEditOutcome {
    Updated,
    Gone,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallerError(String);

#[allow(async_fn_in_trait)]
pub trait PanelInstaller {
    async fn fetch_message(&self, channel: ChannelId, message: MessageId)
        -> Result<PanelPresence, InstallerError>;
    async fn post_message(&self, channel: ChannelId, guild: GuildId, ruleset_key: &str, spec: &PanelSpec)
        -> Result<MessageId, InstallerError>;
    async fn edit_message(&self, channel: ChannelId, message: MessageId, guild: GuildId, ruleset_key: &str, spec: &PanelSpec)
        -> Result<PanelEditOutcome, InstallerError>;
}
```

Both `fetch_message` and `edit_message` distinguish "deleted" (`Gone`, mapped from
Discord 404 / UnknownMessage) from "state unknown" (`Err(InstallerError)`, mapped from
Forbidden / Network / RateLimited). `InstallerError` is opaque (transient detail for
logging), keeping the crate free of `automation-core`.

## spec_hash — explicit presentation projection

Never hash the raw `PanelSpec` (future install-irrelevant metadata would cause
spurious edits; a renderer-only change would leave the hash stale). Hash an explicit
projection instead:

```rust
#[derive(serde::Serialize)]
struct PanelPresentation<'a> {
    render_revision: u32,
    content: &'a str,
    buttons: Vec<ButtonPresentation<'a>>,
}

#[derive(serde::Serialize)]
struct ButtonPresentation<'a> {
    label: &'a str,
    route: String,
}

pub fn spec_hash(render_revision: u32, spec: &PanelSpec) -> String {
    let projection = PanelPresentation {
        render_revision,
        content: &spec.content,
        buttons: spec
            .buttons
            .iter()
            .map(|b| ButtonPresentation {
                label: &b.label,
                route: route_token(&b.route),
            })
            .collect(),
    };
    let bytes = serde_json::to_vec(&projection).expect("panel presentation serializes");
    hex_sha256(&bytes)
}
```

- Included: `content`, button **order**, button `label`, button `route` (a stable
  token, e.g. `static:<key>`), and `render_revision`.
- Excluded: channel binding / resolved `channel_id` (compared separately),
  `panel_key` (in the logical key), ruleset version, `message_id`.
- `render_revision` is a constant owned by the edge (co-located with the custom_id
  codec) and passed into the reconcile. When the codec changes (future RouteId),
  bump it → every panel's hash changes → existing messages are edited, so no stale
  buttons. Button spec fields (`ButtonSpec` = `{label, route}`) carry no style/emoji;
  installer-fixed rendering is covered by `render_revision`.
- No general canonicalizer is needed: the projection has no maps and button order is
  significant and preserved.

`route_token` maps `ButtonRoute::Static { key }` → `static:<key>` and
`ButtonRoute::InstanceAction { action, .. }` → `instance_action:<action>` (install
panels use only `Static`; the `InstanceAction` arm keeps the projection total).
`hex_sha256` is a local `sha2`-based SHA-256 → lowercase-hex helper.

## Reconcile — `install_declared_panels`

```rust
pub async fn install_declared_panels<S, I>(
    guild_id: GuildId,
    ruleset_key: &RuleSetKey,
    ruleset_version: RuleSetVersionId,
    render_revision: u32,
    panels: &[PanelSpec],
    bindings: &ResourceBindingMap,
    store: &S,
    installer: &I,
) -> Result<InstallReport, InstallError>
where
    S: PanelInstallationStore,
    I: PanelInstaller;
```

Per panel, with `key = (guild, ruleset_key, panel.key)`, `desired = spec_hash(render_revision, panel)`,
and `resolved = bindings.channel_bindings.get(&panel.channel)` (`panel.channel` is a
`ResourceKey` → `ChannelId`):

```
resolve channel:
    unresolved -> outcome SkippedUnresolvedChannel (defensive; readiness already validated bindings)

record = store.get(key)?            (store error -> InstallError, fatal, fail-closed)

record is None:
    id = installer.post_message(resolved, guild, ruleset_key, panel)   Err -> SkippedTransient
    store.upsert({version, channel: resolved, message: id, spec_hash: desired})?
    outcome Posted

record exists, record.channel_id != resolved:                          channel binding changed
    id = installer.post_message(resolved, ...)                          Err -> SkippedTransient
    store.upsert({version, channel: resolved, message: id, spec_hash: desired})?
    outcome RepostedNewChannel                                          (old message NOT deleted — limitation)

record exists, same channel:
    presence = installer.fetch_message(record.channel_id, record.message_id)   Err -> SkippedTransient
    presence == Gone:
        id = installer.post_message(resolved, ...)                      Err -> SkippedTransient
        store.upsert({version, channel, message: id, spec_hash: desired})?
        outcome Reposted
    presence == Present, record.spec_hash != desired:
        edit = installer.edit_message(record.channel_id, record.message_id, ...)   Err -> SkippedTransient
        edit == Updated: store.upsert({version, channel, message: same, spec_hash: desired})?; outcome Edited
        edit == Gone:    id = installer.post_message(resolved, ...)?; store.upsert({...message: id, spec_hash: desired})?; outcome Reposted
    presence == Present, record.spec_hash == desired, record.installed_version != ruleset_version:
        store.upsert({same channel/message/hash, installed_version: ruleset_version})?
        outcome PersistenceUpdated
    else:
        outcome NoOp
```

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelAction {
    Posted,
    Reposted,
    RepostedNewChannel,
    Edited,
    PersistenceUpdated,
    NoOp,
    SkippedTransient,
    SkippedUnresolvedChannel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelOutcome {
    pub panel_key: String,
    pub action: PanelAction,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallReport {
    pub outcomes: Vec<PanelOutcome>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallError {
    Store(PanelInstallationStoreError),
}
```

Fail-closed split: **DB/store errors are fatal** (`InstallError::Store` → the bot does
not start, consistent with Postgres-or-die). **Discord transient errors degrade
gracefully** — that panel's outcome is `SkippedTransient` (its record is kept, no
re-post), reconcile continues for the other panels, and the bot starts. The two
no-op forms (`NoOp` vs `PersistenceUpdated`) keep `installed_version` correct across
activation and rollback without a Discord mutation.

## Edge (`automation-runtime`) + tool

- `PANEL_RENDER_REVISION: u32` constant, co-located with the custom_id codec; bumped
  when the codec changes.
- `TwilightPanelInstaller` implements `PanelInstaller`: `fetch_message` via
  `http.message(channel, id)` mapping 404/UnknownMessage → `Ok(Gone)` and
  Forbidden/Network/RateLimited → `Err(InstallerError)`; `post_message` /
  `edit_message` build components, encoding Static button `custom_id`s with the
  existing `encode_button(guild, ruleset_key, key)`; `edit_message` maps 404 → `Gone`.
  The installer performs no policy or hash logic.
- Tool `run`: after readiness passes and before the gateway starts, replace the
  hardcoded `install_panel(...)` with:

  ```
  install_declared_panels(
      guild_id, &runtime.ruleset_key, runtime.version, PANEL_RENDER_REVISION,
      &runtime.definition.panels, &bindings, &installation_store, &installer,
  )
  ```

  `install_panel` and its StudyRoom-specific label/content/button assembly are
  removed. Install runs in the 18c-1 order: DB connect → snapshot → hydrate →
  readiness → **panel reconcile** → gateway. A fatal `InstallError` fails closed
  (gateway not started), matching hydration.

## Postgres store + migration

`ruleset_panel_installations` table, PK `(guild_id, ruleset_key, panel_key)`:
`installed_version BIGINT`, `channel_id TEXT`, `message_id TEXT`, `spec_hash TEXT`
(`^[0-9a-f]{64}$`), `installed_version BETWEEN 1 AND 4294967295`. Discord IDs TEXT.
`PostgresPanelInstallationStore` implements get (`fetch_optional`) and upsert
(`INSERT ... ON CONFLICT (guild_id, ruleset_key, panel_key) DO UPDATE`, single atomic
statement). Row→domain `TryFrom` maps bad persisted values to `Backend` (no panics).
Same root `/migrations`, `sqlx::migrate!`, build.rs rerun-if-changed.

## Testing

- Reconcile unit tests (in-memory store + a scripted mock installer recording
  post/edit/fetch calls and returning configured `PanelPresence`/`PanelEditOutcome`/`Err`):
  - fresh install → `Posted`, store has the record.
  - record + `Present` + same hash + same version → `NoOp`, zero installer mutations.
  - record + `Present` + same hash + newer version → `PersistenceUpdated`, zero
    installer mutations, `installed_version` advanced.
  - record + `Present` + changed hash → `Edited`, one `edit_message`, no post.
  - record + `fetch` returns `Gone` → `Reposted`, one `post_message`, new `message_id`.
  - record + changed hash + `edit` returns `Gone` → `Reposted` (race handled).
  - record + `fetch` returns `Err` → `SkippedTransient`, zero post/edit, record kept.
  - record + `edit` returns `Err` → `SkippedTransient`, record kept.
  - channel binding changed → `RepostedNewChannel`, new `channel_id`+`message_id`.
  - store error → `InstallError::Store` (fatal).
  - `render_revision` bump with identical logical spec → hash differs → `Edited`.
- Ignored Postgres integration for `PostgresPanelInstallationStore` (upsert/get
  round-trip, reconnect durability, PK conflict updates in place).
- `tests/no_ai_gateway.rs` in the new crate; dependency guard (regular deps only).
- Live (reused bot/guild/local `starring`): boot → panel posted once, record written.
  Restart → same message, no duplicate (`fetch` Present, `NoOp`). Delete the message
  in Discord, restart → re-posted (`Reposted`), new `message_id`. Bump the demo panel
  content, restart → `Edited` in place, same `message_id`.

## Roadmap

- 18d-2 Attach-after-register: instance `PostPanel` panels attached to
  `AutomationInstance` resources (atomic ResourcePatch) so cleanup is complete.
- 18d-3 RouteId: compact `custom_id` token; bumping `PANEL_RENDER_REVISION` then
  edits every installed panel to the new codec.
- 18e Durable rollback live: the full arc (pin + dispatch + gated activation +
  durable install) demonstrated end to end.
