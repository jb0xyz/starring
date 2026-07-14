# Intent Recipe Checkpoint Handoff

## Document status

This handoff records the Starring repository and home-server state at the point
where feature expansion stopped on 2026-07-14. It is written for the next AI or
engineer taking over the project.

The repository-level source of truth remains `CURRENT_STATE.md`. This document
adds the detailed implementation, evidence, limitations, and continuation plan
for the Intent IR and Recipe Compiler checkpoint. If later commits change the
implementation, update both documents instead of treating this snapshot as
timeless.

The checkpoint source commit before this handoff document is:

```text
99429f8 feat(eval): add attested Gemma intent checkpoint
```

The feature branch was based on:

```text
origin/main 502c300f18647011ae6d9d9437af15a849a8677d
feat/intent-recipe-compiler
```

## Executive status

Starring now has a working first slice of high-level conversational authoring.
For a supported product capability, Gemma no longer assembles low-level
RuleSet actions. Gemma interprets a user turn and fills bounded semantic slots;
deterministic Rust code owns normalization, defaults, keys, permissions, action
ordering, references, manifests, validation, simulation, and preview.

The only implemented product recipe is:

```text
starring.private_study_room@1
```

The supported default recipe can create a validated preview of a private study
room flow with a launcher, modal, private channel, member role, membership
permissions, creator grant, Help and Join controls, and instance registration.
An explicit any-member Close variant also exists.

This checkpoint is not commercially ready. The architecture and first recipe
are implemented, focused tests are green, and one live Gemma diagnostic
completed successfully. The required clean-source, ten-repeat-per-case Gemma
checkpoint has not been run. Runtime-wide action preflight, external-failure
compensation, production API and identity boundaries, multi-node session
persistence, load testing, and several authoring workflows remain unfinished.

## Product direction

The intended product is a natural conversational design partner, not a
single-shot JSON generator.

- A user may describe a complete feature in one prompt.
- A user may brainstorm first and build later.
- A user may provide requirements incrementally across turns.
- The system should ask only for a genuinely blocking decision that has no safe
  deterministic default.
- Supported capabilities should compile through product-level recipes.
- Supported custom automation should route to a typed planner.
- Capabilities that need state, timers, authorization predicates, or other
  missing runtime primitives should be reported as capability gaps.
- Unsafe requests should be rejected without mutating the Draft.

The long-term direction remains a stateful Discord application and game
builder. That is a separate runtime track. Do not extend the current RuleSet
with ad-hoc counters, timers, or session state. Stabilize the Harness Track,
then introduce a deliberately designed `StatefulSpec` and runtime arc.

## Safety boundary

The central contract has not changed:

```text
AI at design time only
→ deterministic compile
→ validate
→ simulate
→ preview
→ user approval
→ immutable publication
→ approval-bound activation
→ deterministic event-time runtime
```

The authoring model cannot publish, deploy, activate, call Discord, or mutate a
production database. Event-time LLM calls remain forbidden. The new compiler
does not bypass existing approval, version pinning, readiness, publication, or
activation boundaries.

`design-harness` remains a pure library crate. Its regular dependencies do not
include `sqlx`, `rusqlite`, `twilight`, or `ai-gateway`. HTTP and SQLite are
confined to the `design-harness-cli` edge. Candidate execution happens on a
Draft clone; parse, normalization, binding, compilation, validation,
simulation, preview, and root-conflict failures leave the root Draft unchanged.
An SQLite generation conflict reloads durable state before another user turn.
A different persistence failure aborts before the new snapshot becomes durable;
the process-local session may already contain the committed candidate and is
discarded when the CLI exits.

## Implemented architecture

The implemented known-recipe path is:

```text
user conversation
→ one Gemma route or decision extraction
→ IntentWorkspaceV1
→ deterministic normalization and missing-decision resolution
→ ValidatedIntentV1
→ deterministic recipe compiler
→ ordered ScopeRequirement values
→ candidate Draft clone
→ binding-aware validation
→ recipe-specific simulation
→ strict preview
→ root Draft compare-and-swap commit
→ SQLite snapshot generation compare-and-swap
```

Intent IR is an authoring artifact, not a second runtime language.
`InteractionRuleSet` remains the runtime artifact. `ScopeRequirement` remains
the exact postcondition contract for the design harness.

### Intent workspace

Intent schema V1 permits exactly one feature. The model supplies only semantic
fields such as objective, requested outcome, hub channel, locale, copy, naming,
and supported controls. The harness owns:

- intent schema version;
- workspace revision;
- stable feature identifier;
- recipe identity and exact version;
- value provenance;
- generated keys;
- default materialization;
- permission policy;
- raw template construction;
- action order;
- created-resource references;
- instance manifest wiring.

Values preserve provenance such as model-extracted, user-confirmed,
context-derived, and recipe-default. Model extraction is not treated as user
confirmation. The compiler accepts only an opaque normalized intent issued by
the deterministic normalization path.

The exact input hash includes workspace revision and provenance. The semantic
hash excludes revision and provenance but retains user-visible semantics,
stable feature identity, recipe version, locale, copy, naming, controls, and
binding keys. The compiled plan has its own deterministic hash.

### Private study room recipe

The only required external decision is an existing hub-channel binding key.
Locale is `en` or `ko`; English is the deterministic default. Close behavior is
disabled by default. An explicit `any_member` policy is supported.
Creator-only close is not emulated because the runtime lacks the required
authorization predicate.

The default close-disabled recipe produces 22 deterministic operations grouped
into these 14 functional steps:

1. Create the persistent launcher panel and button.
2. Create the room-name modal.
3. Add the modal-open rule.
4. Start the modal-submit rule with an ephemeral defer.
5. Create the member role.
6. Create the private room channel.
7. Deny everyone channel visibility.
8. Allow the created member role channel visibility.
9. Grant the created role to the initiating user.
10. Post the welcome and Help panel in the created channel.
11. Post the Join panel in the hub channel.
12. Register the complete instance footprint.
13. Finish the submit interaction response.
14. Add deterministic Help and Join handlers.

All three rendered buttons have handlers. Four golden traces simulate the
launcher, submit, Help, and Join flows. The explicit any-member Close variant
adds four operations, a fourth rendered button, and a fifth simulation trace.

The close-disabled default plan hash is pinned by deterministic tests:

```text
a0ef96c74dc7605635c960b958dd1ecd47e1ccdf9e5aaeda0f1b771c732b57f1
```

## Gemma session and serving policy

Intent mode is opt-in and is hard-pinned to:

```text
gemma4:12b-mlx
```

Ordinary user turns make exactly one model call and expose exactly one frontier
tool:

- `route_intent_turn` for a new intent turn;
- `resolve_intent_decision` while one deterministic hub decision is pending.

The route schema is a flat discriminated object. The compatibility parser can
restore an older nested representation, but duplicate keys at either level are
rejected before execution. Wrong tools, multiple tool calls, stale revisions,
unpromotable prose, malformed arguments, and transport failures halt after the
single call. Intent mode has no automatic retry or repair loop.

The model-facing anchor includes only the current stage, expected revision,
available channel keys, and current deterministic question. It excludes the
full RuleSet, compiled plan, gate traces, and simulation traces. The system
prompt, conversation, assistant tool call, tool result, and new state anchor are
append-only.

The current edge policy is:

- model: `gemma4:12b-mlx`;
- temperature: `0.1`;
- seed: `0`;
- parallel tool calls: disabled;
- HTTP retries: zero;
- intent transport timeout: 60 seconds;
- default session budgets: 12 model calls, 24 model tool calls, 4 gate
  failures, and 44,000 context characters;
- model preflight: exact model must appear in `/models`;
- response check: served model must exactly equal the requested model.

The operational Ollama context is 16,384 tokens. The evaluation report labels
this as a provider-declared benchmark policy because the OpenAI-compatible
gateway does not expose the active context window. Do not describe it as a
gateway-observed value.

## Conversation and persistence behavior

The durable Intent session stores the workspace, state, active decision,
receipt, Draft, transcript, and exact binding fingerprint. Restore recomputes
the fingerprint from both binding keys and resolved Discord identifiers and
fails closed on any mismatch.

SQLite persistence uses a monotonically increasing generation. A save succeeds
only when the stored generation still equals the loaded generation. Two writers
that load the same generation cannot both commit. Interactive conflict handling
discards the in-memory result, reloads durable state, and requires a fresh user
turn instead of replaying an old model result.

The evaluation restart case writes the session to a temporary SQLite file,
closes the connection, reopens it, deserializes the snapshot, and reconstructs
the session. This proves an SQLite close/reopen boundary. It does not prove a
separate operating-system process restart.

Snapshot V5-to-V6 migration currently exists in the CLI SQLite store. An
external caller using `DesignSession::restore` directly cannot migrate a V5
snapshot through that library API.

Current conversation limitations:

- `discussion` is retained in the transcript but does not update a structured
  brainstorming workspace.
- `typed_planner` returns a routed outcome but does not actually transfer into
  a typed-planner session.
- `PreviewReady` has no recipe-owned-region edit and recompile workflow.
- `working_draft` currently still runs validation and simulation and lands in
  `PreviewReady`; it is not a separate incomplete-draft terminal state.
- Intent transcript compaction is not implemented. The session halts when its
  character budget is exceeded.

## Runtime hardening implemented so far

Runtime responsibilities have been split into preparation, effect, and state
modules without changing the public execution contract.

Modal input now supports additive contracts:

- `min_length`;
- `max_length`;
- `input_policy` with `Preserve` and `TrimUnicodeWhitespace`;
- UTF-16 code-unit length checks;
- min in `0..4000`;
- max in `1..4000`;
- min not greater than max;
- unknown-input rejection;
- required missing or empty rejection;
- absent optional values normalized to an empty string;
- validation before defer, response, Discord mutation, or state mutation;
- Twilight presentation of min and max bounds;
- legacy serialization and content-hash preservation.

`update_modal` preserves the existing same-key field contract. The model-facing
modal authoring tool and `ScopeModalField` cannot yet author these contracts.
The V1 room-name field is required but still inherits the legacy maximum and
preserve policy. Recipe V2 must pin a narrow input contract and downstream
template and name budgets.

Legacy modal documents keep their old wire shape and content hash. A new V1
document that actually emits the optional modal contract fields can still be
rejected by an older strict `deny_unknown_fields` reader. Plan software rollout
and rollback compatibility before publishing bounded V1 documents, or prefer a
new pinned recipe and schema strategy.

The largest runtime safety gap remains whole-plan deterministic preflight.
Actions are still prepared and executed one by one. A late deterministic
failure can therefore occur after an earlier external mutation. Existing tests
make this limitation explicit: a role mutation may succeed before a later panel
template fails. External failures also lack provisioning state, compensation,
reconciliation, and replay idempotency.

## Evaluation checkpoint

The Intent checkpoint is a separate Promptfoo cohort using evaluation input
schema 3. It always requests `gemma4:12b-mlx`, disables cache, fixes concurrency
at one, and forbids an alternate prebuilt harness binary. Before each sample it
runs `cargo build --locked` from the current source, records the source and
embedded build commit and dirty state, hashes the executable, and requires the
child process to match those source and binary identities before a model call.
The final acceptance checks, rather than the pre-build step, require the source
and embedded build identities to be clean.

This is local unsigned provenance, not signed supply-chain attestation.
The model check pins the exact requested and gateway-reported model identifier
`gemma4:12b-mlx`; it does not cryptographically identify model weights, the
Ollama artifact, quantization, or server configuration.

The exact ten cases are:

1. English complete one-shot.
2. Missing hub decision over two turns.
3. Pending decision across an SQLite close/reopen.
4. Korean complete one-shot.
5. Discussion followed by build.
6. Typed-planner fallback.
7. Creator-only close capability gap.
8. Stateful game capability gap.
9. Direct live-mutation rejection.
10. Secret-disclosure rejection.

For the close-disabled recipe checkpoint, acceptance requires:

- at least ten samples for every exact case;
- all Promptfoo row assertions passing;
- at least 90% known-recipe selection per case;
- 100% actual validation and simulation gates on selected recipes;
- exactly 22 deterministic operations on selected recipes;
- no unnecessary question for complete requests;
- exact missing-decision resolution;
- exact mutation-free fallback routing;
- one model call and one model tool call per turn;
- no oracle or injected plan;
- stable identities within each repeated known-recipe case;
- one-shot, confirmed multi-turn, restored multi-turn, and
  discussion-then-build semantic-hash, plan-hash, and RuleSet equivalence;
- different exact input hashes for one-shot and user-confirmed multi-turn due to
  provenance, with the SQLite roundtrip preserving the confirmed input hash;
- preview latency below 8 seconds at P50 and 20 seconds at P95;
- every turn below the 60-second hard boundary;
- non-overlapping sample timestamps and one consistent metadata cohort.

The checkpoint deliberately does not cover:

- the any-member Close variant;
- custom copy and naming overrides;
- an independent Intent V3 postcheck;
- natural-language stale-revision and target-conflict cases;
- a separate-process restart;
- concurrent multi-writer load;
- throughput, queueing, and backpressure;
- deterministic whole-plan preflight;
- compensation or external Discord failure recovery;
- production identity, approval UI, publication, or activation.

Passing this checkpoint would certify the asserted close-disabled structural,
routing, gate, equivalence, call-count, and latency properties. It would not
independently certify that hub, locale, objective, or copy semantics match each
natural-language request. It would not certify model weights or server
configuration, and it would not certify commercial readiness.

### Current evidence

Focused pre-handoff checks completed before this document:

- `cargo test -p design-harness-cli`: 61 tests passed;
- `design-harness-cli` dependency guard: passed;
- `cargo clippy -p design-harness-cli --all-targets -- -D warnings`: passed;
- `npm --prefix eval/design-harness run check`: 34 JavaScript tests and both
  Promptfoo configuration validations passed;
- `cargo fmt --all -- --check`: passed;
- source build through the evaluation provider path: passed.

The complete workspace gates and GitHub Actions must still pass on the final
handoff commits before merge.

No clean-source ten-repeat-per-case Intent cohort has been run. Do not infer
reliability or latency acceptance from unit tests.

One earlier live Gemma diagnostic, before the final evaluation framework, used
a complete English private-room request and produced:

- one model call;
- one model tool call;
- no clarification;
- route `private_study_room`;
- hub `study_hub`;
- Draft revision 22;
- one panel, one modal, four rules, and fifteen actions;
- validation and simulation stamps at revision 22;
- 22 compiled operations;
- input hash
  `7f528aa013b90e68017f16785c7f0c95d37a92de223c22ed115cba1f8299d3d7`;
- semantic hash
  `2afc4ddac82770983ff2455e466ebc3778f30b8eb538d10feb5a8d6bbfd7ab52`;
- plan hash
  `a0ef96c74dc7605635c960b958dd1ecd47e1ccdf9e5aaeda0f1b771c732b57f1`.

This is a diagnostic sample, not an acceptance cohort.

## Home-server operational snapshot

This is a point-in-time snapshot from a 2026-07-14 KST audit, not repository
state. The exact collection time was not recorded:

- Mac mini memory: 24 GiB;
- Ollama: `0.31.1`;
- loaded model: `gemma4:12b-mlx`;
- loaded footprint reported by Ollama: about 11 GB, GPU 100%, context 16384,
  keep-alive forever;
- model artifact: about 7.7 GB;
- Ollama bind: `127.0.0.1:11434`;
- authenticated local gateway: `127.0.0.1:18080`;
- authenticated public gateway: `https://llm-api.starring.co.kr/v1`;
- unauthenticated `/models` returned 401 locally and publicly;
- `OLLAMA_KEEP_ALIVE=-1`;
- flash attention enabled;
- KV cache type `q8_0`;
- `OLLAMA_NUM_PARALLEL=1`;
- max queue 8;
- max loaded models 1;
- launch agents running: `local.ollama.server`, `local.llm-api`, and
  `local.cloudflared.starring`.

The gateway credential is stored outside Git in macOS Keychain under service
`com.starring.llm-api-key` and account `llm-api`. Never copy its value into a
prompt, document, result artifact, command history, source file, commit, issue,
or pull request.

The snapshot also showed approximately 85% memory-pressure availability,
3.85 GiB of 5 GiB swap in use, load averages around 1.42, 1.85, and 2.31, and
about 108 GiB disk available. Re-measure before drawing a capacity conclusion.

Other model artifacts were still present on disk:

- `qwen3.5:9b-mlx`;
- `ornith:9b`.

Gemma-only means the serving, implementation, and acceptance policy. It does
not mean those artifacts were deleted. Never mix their output into a Gemma
checkpoint.

Operational files outside the repository:

```text
/Users/jungbogeon/Library/LaunchAgents/local.ollama.server.plist
/Users/jungbogeon/Library/LaunchAgents/local.llm-api.plist
/Users/jungbogeon/Library/LaunchAgents/local.cloudflared.starring.plist
/Users/jungbogeon/Services/llm-api/server.mjs
/Users/jungbogeon/Services/cloudflared-starring/run.sh
```

## Known gaps and risk register

### Must resolve before a commercial pilot

1. Run and preserve the clean-source Gemma Intent cohort; diagnose every
   failure without weakening assertions.
2. Preflight every deterministic action before the first Discord side effect.
3. Introduce instance-scoped channel and role naming.
4. Pin a V2 recipe with bounded modal input, trimming semantics, and downstream
   template and name budgets.
5. Persist provisioning state and implement compensation, reconciliation, and
   replay idempotency for uncertain external effects.
6. Provide authenticated user and guild-binding authority.
7. Connect preview to the existing approval, publication, and activation
   boundary without creating a bypass.
8. Define load, queue, backpressure, timeout, and availability SLOs around the
   single loaded model and `NUM_PARALLEL=1` policy.

### Authoring product gaps

1. Actual typed-planner handoff after the `typed_planner` route.
2. Structured brainstorming state instead of transcript-only discussion.
3. Recipe-owned-region edit and deterministic recompile after preview.
4. A meaningful working-draft state distinct from validated preview.
5. Bounded Intent transcript compression with durable summaries.
6. Recipe catalog expansion only after the first recipe is reliable.
7. Independent semantic fidelity checks for requested hub, locale, objective,
   copy, naming, and capability identifiers.

### Evaluation and provenance maintenance gaps

1. `build.rs` Git ref watching needs hardening for symbolic refs, worktrees, and
   dependency-only commits. Current mismatches fail closed but may require a
   manual clean rebuild.
2. The evaluation provider assumes the default `target/debug` location and
   does not support a custom `CARGO_TARGET_DIR`.
3. Some acceptance classification is driven by YAML case flags. Add a static
   case-metadata manifest and more negative tests.
4. Add the any-member Close variant, copy and naming overrides, stale/conflict
   prompts, an independent V3 postcheck, separate-process restart, and
   multi-writer cases as separately named checkpoints.
5. Local unsigned build identity is not a signed supply-chain attestation.
6. Model identity is an exact gateway tag check, not a weights, artifact, or
   server-configuration digest.

### Maintainability debt

1. `tools/design-harness/src/main.rs` is about 2,184 lines. Split interactive,
   evaluation, and provenance responsibilities before adding more command
   behavior.
2. `intent/normalize.rs` is about 755 lines. Separate generic normalization,
   recipe resolution, and patch/revision behavior before Recipe V2.
3. `intent/private_study_room.rs` is about 510 lines. Separate stable recipe
   contract, lowering, naming, copy defaults, and operation construction before
   adding variants.
4. Keep feature and safety changes in independent commits.
5. Preserve dependency guards and the pure-core/edge-adapter split.
6. Replace the direct-manifest dependency denylist with a resolved-graph guard
   before relying on it as the only transitive purity proof.
7. Treat `ModalFieldSpec`, new public enum variants, and the private
   `SessionSnapshot` field as source-compatibility changes before external crate
   consumers exist. The current internal `0.1.0` workspace builds together, but
   external struct literals and exhaustive matches would need migration.

## Prioritized continuation plan

The next AI should continue in this order unless new evidence invalidates it.

### Phase 1: Establish the measured Gemma baseline

1. Start from clean `main` after this checkpoint merge.
2. Confirm the home-server services and exact `gemma4:12b-mlx` model.
3. Run the exact ten-case Intent cohort with at least ten samples per case.
4. Store raw output outside source control unless the repository policy is
   deliberately changed.
5. Run the summarizer and acceptance script unchanged.
6. Record selection, gate, stability, equivalence, call-count, and latency
   failures case by case.
7. Treat the result as a close-disabled recipe checkpoint only.

Do not tune prompts against hidden assertion details, add an oracle, reduce the
sample count, mix models, reuse cached responses, or weaken acceptance to make
the report pass.

### Phase 2: Remove deterministic orphan-resource failures

1. Specify a complete prepared-action representation.
2. Resolve references and render every template before any side effect.
3. Validate Discord limits, permission inputs, modal values, and instance
   manifest completeness in the preflight phase.
4. Prove that a late deterministic failure causes zero responder, Discord, or
   state mutation.
5. Commit the behavior-preserving split separately from the new preflight
   behavior.

### Phase 3: Ship Private Study Room Recipe V2

1. Decide raw-versus-trimmed modal length semantics.
2. Pin exact room-name min, max, and normalization policy.
3. Prove every derived channel name, role name, response, and panel content fits
   its downstream Discord limit.
4. Add instance identity to generated resource names without exposing raw IDs
   to the model.
5. Preserve Recipe V1 as an immutable version and compile new behavior under
   Recipe V2.
6. Add deterministic migration and compatibility tests rather than changing V1
   in place.

### Phase 4: Handle uncertain external effects

1. Define provisioning states and durable operation receipts.
2. Make retries idempotent across process restart.
3. Add compensating cleanup for partially created roles, channels, messages,
   grants, and overwrites.
4. Add reconciliation for Discord success with lost local acknowledgement.
5. Prove replay safety under timeout, rate limit, forbidden, not found, network
   interruption, and process crash.

### Phase 5: Complete the authoring experience

1. Implement real typed-planner session handoff.
2. Add structured brainstorming decisions and explicit user confirmation.
3. Implement recipe-owned-region edit and recompile with stable ownership.
4. Add context summarization without dropping unresolved decisions or safety
   history.
5. Introduce the production API only after authenticated actor, guild binding,
   session ownership, preview, approval, publication, and activation contracts
   are explicit.

### Phase 6: Capacity and product expansion

1. Measure cold, warm, single-request, queued, and concurrent latency on the
   actual Mac mini.
2. Define queue admission and overload responses for `NUM_PARALLEL=1`.
3. Measure memory, swap, CPU/GPU pressure, and backend co-residency during the
   full cohort and a sustained load test.
4. Expand the deterministic recipe catalog one product capability at a time.
5. Begin the separate Stateful Runtime Track only after the Harness Track has a
   reliable production boundary.

## Commit map for this branch

The branch intentionally used feature-level commits:

```text
8d0cae1 docs(design-harness): specify intent recipe architecture
442ff86 feat(design-harness): add normalized intent workspace
fe601a9 feat(design-harness): compile private study room intents
07f0ac3 refactor(design-harness): isolate private room recipe expansion
d877250 feat(design-harness): make candidate gates binding aware
26e3aba refactor(design-harness): isolate atomic plan execution
7f1484f feat(design-harness): prepare intent candidates atomically
b9bd46a refactor(design-harness): split recipe simulation responsibilities
0e87a86 feat(design-harness): preserve resumable intent workspaces
5c54f46 feat(design-harness): add semantic intent fingerprints
e7f9dbc feat(design-harness): add single-call intent frontiers
b68e098 docs(design-harness): specify intent session serving
ff67b89 refactor(automation-core): separate execution preparation
6a12850 feat(design-harness): add durable intent recipe sessions
fee08ff feat(design-harness-cli): serve gemma intent sessions
050de73 fix(design-harness): flatten gemma intent routing
5528cec fix(design-harness): preserve strict intent routes
8bb30b5 feat(automation-core): enforce modal input contracts
99429f8 feat(eval): add attested Gemma intent checkpoint
```

## Core file map

Direction and design:

- `docs/superpowers/specs/2026-07-12-harness-vision-and-sequencing.md`
- `docs/superpowers/specs/2026-07-14-intent-ir-recipe-compiler-design.md`
- `docs/superpowers/specs/2026-07-14-intent-recipe-session-serving-design.md`

Intent core:

- `crates/design-harness/src/intent/model.rs`
- `crates/design-harness/src/intent/proposal.rs`
- `crates/design-harness/src/intent/normalize.rs`
- `crates/design-harness/src/intent/semantic.rs`
- `crates/design-harness/src/intent/compile.rs`
- `crates/design-harness/src/intent/private_study_room.rs`
- `crates/design-harness/src/intent/candidate.rs`
- `crates/design-harness/src/intent/simulation/`

Conversation and session:

- `crates/design-harness/src/turn/intent_protocol.rs`
- `crates/design-harness/src/session/intent_routing/mod.rs`
- `crates/design-harness/src/session/intent_routing/state.rs`
- `crates/design-harness/src/session/intent_routing/execute.rs`
- `crates/design-harness/src/session/snapshot.rs`

CLI edge:

- `tools/design-harness/src/client.rs`
- `tools/design-harness/src/config.rs`
- `tools/design-harness/src/store.rs`
- `tools/design-harness/src/eval.rs`
- `tools/design-harness/src/main.rs`
- `tools/design-harness/build.rs`

Evaluation:

- `eval/design-harness/promptfooconfig.intent.yaml`
- `eval/design-harness/intent-cases.yaml`
- `eval/design-harness/intent-assertions.js`
- `eval/design-harness/acceptance.js`
- `eval/design-harness/summarize.js`
- `eval/design-harness/README.md`

Runtime hardening:

- `crates/automation-state/src/modal.rs`
- `crates/automation-core/src/modal_input.rs`
- `crates/automation-core/src/prepare.rs`
- `crates/automation-core/src/execution/`

## Safe environment contract

The following names are configuration interfaces. Values must stay in the
environment or Keychain and must never be committed:

```text
STARRING_LLM_API_KEY
STARRING_LLM_BASE_URL
STARRING_LLM_MODEL
STARRING_HARNESS_MODE
STARRING_HARNESS_DB_PATH
STARRING_HARNESS_SESSION_ID
STARRING_HARNESS_BINDINGS_JSON
STARRING_HARNESS_MAX_MODEL_CALLS
STARRING_HARNESS_MAX_TOOL_CALLS
STARRING_HARNESS_MAX_GATE_FAILURES
STARRING_HARNESS_CONTEXT_CHARS
STARRING_EVAL_DECLARED_CONTEXT_TOKENS
STARRING_EVAL_GATEWAY_ID
STARRING_EVAL_RUN_ID
STARRING_EVAL_RUN_ORDER
STARRING_EVAL_TIMEOUT_MS
```

The CLI rejects a non-Gemma `STARRING_LLM_MODEL`. The Intent checkpoint also
forbids `STARRING_HARNESS_BIN` so a stale alternate executable cannot silently
enter the cohort.

## Verification commands

Run from the repository root with the active Rust toolchain directory on
`PATH`:

```sh
TOOLCHAIN_BIN="$(dirname "$(rustup which cargo)")"
PATH="$TOOLCHAIN_BIN:$PATH" cargo fmt --all -- --check
PATH="$TOOLCHAIN_BIN:$PATH" cargo build --workspace --all-targets
PATH="$TOOLCHAIN_BIN:$PATH" cargo test --workspace
PATH="$TOOLCHAIN_BIN:$PATH" cargo clippy --workspace --all-targets -- -D warnings
PATH="$TOOLCHAIN_BIN:$PATH" cargo build -p interaction-smoke --features unsafe-dev-activation
npm --prefix eval/design-harness ci
npm --prefix eval/design-harness run check
git diff --check
```

The PostgreSQL CI job runs the six ignored adapter and dispatch/readiness test
packages serially. No live Discord or live LLM call belongs in ordinary CI.

The Gemma checkpoint command is documented in
`eval/design-harness/README.md`. Run it only from clean committed source with
the credential provided from Keychain or a protected environment. Keep raw
results free of secrets and inspect them before sharing.

## Resume checklist

Before changing code, the next AI should:

1. Read `CURRENT_STATE.md` and this handoff completely.
2. Read the two 2026-07-14 Intent design documents.
3. Confirm `git status`, current `main`, and whether and which checkpoint commit
   was merged.
4. Confirm no credential or result artifact is tracked.
5. Run the local static gates.
6. Inspect the home-server model and gateway without printing credentials.
7. Run the Gemma cohort before attempting prompt or router changes.
8. Open one bounded implementation task at a time.
9. Commit each feature, refactor, evaluation change, and safety change
   independently.
10. Record evidence and limitations honestly after every checkpoint.

The first new code change should be selected from measured Gemma failures or
the deterministic whole-plan preflight gap. Do not add another recipe merely
to demonstrate breadth.
