# AutomationSpec V1

`AutomationSpec V1` is the first recipe-independent, typed authoring contract. Natural-language
authoring may propose this document, but the model cannot execute code, issue Discord requests, or
mutate an installation. The platform validates and canonicalizes the document before it can be
previewed, simulated, or compiled into the existing immutable interaction RuleSet path.

The wire identity is the exact pair `schema_version: 1` and
`kind: "starring.automation-spec.v1"`. V1 owns its request DTOs and lowers them explicitly into
runtime types, so later runtime changes cannot silently reinterpret the V1 wire contract.

## Closed surface

V1 exposes three triggers:

- `button_click`
- `modal_submit`
- `instance_action`

It exposes six pure conditions: `always`, `input_non_empty`, `input_equals`, `all`, `any`, and
`not`. Modal conditions may reference only fields declared by the modal that triggered the
workflow. Simulation evaluates them after the same required-field, optional-field, Unicode trim,
and UTF-16 length normalization used by the live interaction runtime.

The action set is closed to the current interaction primitives: role grant, ephemeral response,
modal open, channel and role creation, permission overwrite, panel post, deferred response,
response edit, instance registration, and instance teardown. Permission values are a closed enum;
raw permission bits are not accepted.

Arbitrary code, arbitrary HTTP, event-time LLM calls, secrets, loops, and recursion are not part of
the contract. Collections, encoded size, conditions, action count, panel and modal wire shapes,
Discord custom IDs, templates, names, aliases, response sequencing, trigger uniqueness, and
simulation inputs all have fixed bounds. The existing structural RuleSet validator is also run on
the explicitly lowered graph.

## Identity and compilation provenance

Canonical compact JSON is hashed with a domain-separated, length-framed SHA-256 digest. A
successful unconditional compilation produces four distinct identities:

1. the canonical AutomationSpec digest;
2. the official immutable RuleSet content hash;
3. a canonical source map from workflow and action-node IDs to RuleSet rule/action indexes;
4. a compilation binding that commits the source identity, target identity, compiler revision,
   and source-map digest.

The source map is a full one-to-one mapping in V1. Validation recomputes the RuleSet and all
identities, and rejects missing, duplicate, reordered, or tampered mappings. A future compiler
that expands one authored node into several runtime nodes requires a new source-map version.

Conditions are previewable and simulatable but the current interaction runtime does not yet
evaluate them. A conditional spec therefore reports `runtime_extension_required`, includes the
`conditional_execution_runtime_unavailable` blocker, and does not expose a compiled target,
source-map digest, or binding digest. It must never be deployed as an unconditional RuleSet.

## Preview and simulation meaning

The preview reports static contract eligibility, conservative Discord permission requirements,
capabilities, deployment-time panel posts, bounded per-event effects, and maximum actions per
event. It deliberately reports activation readiness and panel-installation readiness as
`not_evaluated`, and event execution readiness as `input_and_snapshot_dependent`.

Those labels are important: guild bindings, channel-effective permissions, role hierarchy,
installation journal capacity, active instance state, live Discord limits, and concurrent drift
cannot be proven from a context-free spec. Apply-time contextual readiness remains authoritative.

Simulation accepts a post-gateway-admission event fixture. Duplicate raw modal input keys and raw
interaction payload framing are handled by the gateway before this stage; the simulator still
enforces bounded keys, values, count, and total payload size, then reuses live modal normalization.
Its trace contains the matched workflow, normalized inputs, condition result, and stable authored
action-node IDs. Simulation does not call Discord, write a database, or execute effects.

## Read-only authoring API

The product HTTP edge publishes three installation-scoped endpoints:

- `GET /v1/installations/{installation_id}/authoring/automation-spec/descriptor`
- `POST /v1/installations/{installation_id}/authoring/automation-spec/previews`
- `POST /v1/installations/{installation_id}/authoring/automation-spec/simulations`

All three require a valid session and freshly verified `Author` authority for the installation.
The two POST endpoints also require the normal exact-Origin and session-bound CSRF boundary. They
do not require an idempotency key because they are pure computations and create no durable state.

Malformed or unknown-field JSON is a transport error. A well-formed but semantically invalid spec
or event returns a structured, stable diagnostic response with `valid: false`. The descriptor
separates platform support from per-installation readiness and binds its closed primitive catalog
and limits to a descriptor digest.

These endpoints do not persist specs, return raw compiled RuleSets, create promotion IDs, approve,
apply, deploy, or contact connectors. The existing authoring session to promotion, approval, and
Apply path remains the only mutation path.

## Planned runtime increments

1. Add the version-pinned event envelope and deterministic condition evaluator.
2. Add a separate stateful execution artifact with transactional state changes and outbox effects.
3. Add bounded authorization, cooldown, counters, and state transitions.
4. Add durable one-shot timers, then bounded recurring schedules and migration rules.
5. Add first-party connector capability manifests before considering any general egress surface.

The thin web console currently uses the descriptor for a fresh-authorized installation connection
check and renders the existing safe conversation projection without recipe-name branches. A typed
AutomationSpec editor plus preview and simulation trace renderer is the next UI increment; it must
consume the descriptor and preserve an explicit unknown-node fallback rather than adding recipe
specific branches.
