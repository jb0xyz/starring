> Historical checkpoint: current Luna V4 acceptance evidence and continuation
> live in
> `docs/superpowers/handoffs/2026-07-17-luna-v4-acceptance-hardening-handoff.md`.
> Preserve the body below as the pre-certification Intent V4 record.

# Intent V4 Semantic Identity Handoff

> Serving update, 2026-07-17: the Gemma-only evaluation and continuation
> instructions in this handoff are historical. Active serving is the private
> Luna-medium Codex worker described in `CURRENT_STATE.md` and
> `docs/superpowers/runbooks/2026-07-17-macos-codex-worker-operations.md`.
> Do not restart or use the retired Gemma path for current acceptance evidence.

## Document status

This handoff records the implemented Intent V4 semantic-identity checkpoint and
the exact remaining path to a main merge. It supersedes the current-state and
continuation guidance in `2026-07-15-gemma-intent-cohort-handoff.md`. That older
handoff remains the authoritative record of the measured V3 Gemma samples and
must not be rewritten as V4 evidence.

The repository-level map remains `CURRENT_STATE.md`. The normative V4 contract
is `docs/superpowers/specs/2026-07-15-intent-v4-semantic-identity-design.md`.
Where this handoff summarizes a boundary, the spec and deterministic tests own
the exact field, digest, and replay behavior.

This document describes the branch before its live V4 Promptfoo cohort, PR, CI,
and main merge. Those items are explicitly pending. Do not infer a passing live
V4 model result from the deterministic implementation or the historical V3
rows.

## Exact repository checkpoint

```text
repository: /Users/jungbogeon/starring
branch: feat/intent-v4-semantic-identity
main merge base: 95b6d80dfb284a53bd2efcc78fd503a43cbb3906
semantic-grounding checkpoint: 4254713 fix(intent): harden V4 semantic grounding
maintenance checkpoint at handoff drafting: f37cc38 refactor(intent): split boundary evidence
```

The V4 branch starts at `95b6d80` and contains the V4 design, identity,
evidence, serving, restore, evaluation, grounding, and maintenance work. The
most useful orientation commits are:

```text
c11a826 docs(intent): specify V4 semantic identity boundaries
dd51f14 feat(intent): bind V4 semantic identity and evidence
eec4d3a test(eval): enforce V4 identity and serving evidence
686c692 feat(intent): ground V4 safety and detail semantics
0d13f05 feat(intent): stabilize V9 extraction context
ec585a3 feat(intent): harden semantic identity restore
513c9ea test(eval): enforce semantic identity classes
4415ef6 docs(intent): specify durable semantic identity
e587879 fix(intent): bound complete discussion responses
a61e45a fix(intent): bind serving revision in harness
7b238d6 feat(intent): reconcile capability evidence
0c5b692 feat(intent): ground runtime from current human
c258d40 perf(intent): bound grounding replay work
d20cae6 refactor(intent): split grounding responsibilities
3a92d55 feat(eval): record model finish reasons
fe65200 perf(intent): bind detail extraction to active literals
3c9184b refactor(intent): reuse grounded detail ticket
819ce3e fix(intent): ground runtime outside model frontier
cbda03a test(intent): pin V4 identity relationships
c17209e fix(intent): align V12 serving contract
4254713 fix(intent): harden V4 semantic grounding
d401e03 refactor(eval): remove stale normalizer version labels
3e896db refactor(intent): split safety control grammar
8b272a6 refactor(intent): isolate metalinguistic scope
e15e570 refactor(intent): split boundary classification
f37cc38 refactor(intent): split boundary evidence
```

The branch may have advanced when another AI reads this document. Always run:

```sh
git status --short --branch
git log --reverse --oneline "$(git merge-base main HEAD)..HEAD"
git diff --stat main...HEAD
```

before changing or publishing it. Do not discard an existing dirty worktree.
At handoff drafting, all planned responsibility splits were committed, the
post-split full deterministic local gates were green, and only the two
handoff-document changes were dirty.

## Executive status

Intent protocol V4 is implemented for the first managed recipe:

```text
starring.private_study_room@1
```

The central V3 defect is removed structurally. A model-authored freeform
`objective` no longer participates in the active build wire or authoritative
identity. Human evidence, route semantics, normalized compiler input,
revision-independent semantic intent, deterministic plan, candidate RuleSet,
Draft stage, and transcript integrity now have explicit separate roles.

The model remains a bounded semantic extractor. Deterministic Rust code owns
request-mode reconciliation, safety and capability adjudication, runtime and
detail grounding, recipe selection, defaults, recipe compilation, candidate
construction, validation, exact simulation, preview, persistence checks, and
restore replay. The model never receives a publish, deploy, activation,
Discord, production-database, approval-bypass, or secret-disclosure tool.

The deterministic implementation and identity matrix are present. A focused
V4 library run passed 648 tests. After the behavior-preserving module splits,
the full Rust workspace, workspace clippy, formatting, JavaScript and Promptfoo
static suite, diff checks, and scope and safety audits were green locally.
GitHub CI and the clean-source live V4 Promptfoo cohort have not been completed
or recorded yet.

## Active request flow

```text
current human turn
→ one interpret_intent_core call
→ strict V4 Core without objective
→ deterministic current-human grounding
   ├─ request mode and preview outcome
   ├─ safety-boundary roles
   ├─ runtime requirements
   ├─ mandatory-control restatements
   ├─ exact capability evidence
   └─ selected recipe-detail facets
→ deterministic capability and safety adjudication
   ├─ discussion
   ├─ rejection
   ├─ capability gap
   ├─ typed-planner classification
   └─ pinned private-study-room recipe
      ├─ no selected detail facet
      │  → deterministic defaults
      └─ selected copy, naming, or controls facet
         → one isolated detail call
         → exact served-schema parse
         → slot-specific current-human literal binding
→ typed workspace preparation
→ deterministic Recipe Compiler
→ atomic Draft candidate
→ validate
→ exact recipe simulation
→ preview-ready receipt
```

Default, discussion, fallback, capability-gap, and rejection paths use exactly
one model call and one model tool call. A successful managed-recipe request with
explicit `copy`, `naming`, or `controls` detail uses exactly one additional
isolated call, for two calls and two tool calls total. There is no automatic
model retry, repair, router call, or event-time model dependency.

## V4 identity axes

V4 does not treat one hash as proof of every property. Each axis answers a
different question.

| Axis | Authoritative content | Deliberate exclusions | Required relationship |
|---|---|---|---|
| Request evidence | Ordered accepted human turns, revisions, transcript indexes, decision IDs, option digest, accepted typed value | Model output, rejected or stale answers | One-shot and clarification may differ; clarification and restart remain continuous |
| Route semantic identity | Closed request mode, automation kind, requested outcome, hub, locale, close authorization, grounded runtime and boundary requirements, exact unmapped capabilities, selected detail facets | Request evidence, model response, tool IDs, revision metadata | Equivalent Core semantics converge even when human wording differs |
| Audited adjudication | Route semantic digest plus initial evidence head, manifest identity, blockers, violations, and route target | Display prose | Same route semantics may have different adjudication when evidence provenance differs |
| Compiler input identity | Normalized validated intent, revision, and value provenance | Raw turns, transcript, evidence, response prose, display labels | Revision or provenance differences may remain visible |
| Semantic intent identity | Schema, requested outcome, feature, recipe/version, resolved hub, locale, copy, naming, controls, authorization | Revision, provenance, evidence, display, model response, compiler metadata | Equivalent one-shot, clarification, restart, and future typed-preference results converge |
| Compiled plan identity | Ordered deterministic `ScopeRequirement` list in a domain-separated canonical digest | Evidence, display, Draft stamps | Changes exactly when executable requirements change |
| Candidate RuleSet identity | The actual RuleSet that passed validation and simulation | Draft revision, gate stamps, summaries | Binds the preview artifact rather than assuming the plan implies it |
| Candidate Draft and stage binding | Complete Draft state and authoritative preview or awaiting-decision stage | None of the persisted stage state | Detects inconsistent partial persistence edits |
| Transcript integrity | Exact complete append-only transcript bytes with fixed-width length framing | No byte normalization | Detects insertion, deletion, reorder, role, content, call, tool, or raw-argument byte drift |

The first six semantic/provenance axes must not be collapsed merely because
some requests produce equal values. The candidate-Draft, stage, and transcript
bindings are integrity defenses, not new product semantics.

## Human-grounded authority

### Request mode and requested outcome

The model cannot unilaterally turn copied text, discussion, or a hold into a
build. A deterministic quote-aware unit scanner evaluates the current human
turn. It recognizes bounded direct English and Korean construction forms,
target-aware positive builds and negative holds, direct discussion forms, and
direct validated-preview or working-draft preferences.

A build records every recognized alias in its direct object. A later hold for
the active target retracts the build; a hold for an absent subtarget remains a
constraint instead of canceling the whole request. Copied payloads, quoted
examples, hypotheticals, grammatical descriptions, UI-copy suffixes, and
unmatched quote state carry no build or preview authority. A copied block ends
only at a closed standalone terminator. Closed payload-analysis commands select
discussion but do not release copied commands for execution.

Grounded discussion clears build, channel, runtime, unmapped, and detail
semantics before adjudication. Grounded build clears model discussion prose.
Discussion presentation is non-authoritative, control-validated, capped at 480
UTF-16 code units, and rejected rather than truncated when overlong.

### Locale and close authorization

The active Core schema no longer asks the model to author locale or room-close
policy. Legacy transcript arguments may still carry both compatibility fields,
but serving and restore replace them from the current human turn before
normalization. Missing language becomes `unspecified`; missing close intent
becomes `not_requested`. Direct English and Korean defaults, explicit disabled,
any-member, and creator-only close forms are recognized. Quoted copy,
hypotheticals, detectors, general ticket-closing behavior, negated locale use,
non-exclusive creator mentions, alternatives, and conflicts cannot silently
select these recipe axes. Correction and alternative authority survives only
the immediately connected clause, and the scanner keeps constant state with
linear measured work.

The existing crate-root Core parser remains a one-argument structural
compatibility boundary. It cannot establish serving semantics from
model-authored hidden fields alone. Serving and replay use the crate-internal
human-aware parser, which always replaces those fields from the current human
turn before normalization.

### Safety boundaries

Safety requests are derived from current-human language rather than accepted
from model fields. The grounder separates:

- requests to bypass validation, preview, or user approval;
- requests for direct live Discord mutation;
- requests to disclose secret values.

The implementation uses shared quote-aware, bounded, linear scanners. Gate
grammar has closed actions, targets, modifiers, and scope. Action polarity is
local to the final sixteen complete tokens, with closed preservation,
prohibition, permission, and sequential/additive continuation rules. Quoted UI
copy and metadata discussion have no execution authority. An operative
conditional treats the antecedent as context and the consequent as the active
request; an unsafe condition followed by a safe notification is not converted
into an unsafe action, while an unsafe consequent still rejects. English and
Korean no-comma conditions and multi-antecedent forms have deterministic tests.

Secret grounding distinguishes secret metadata from the secret value, resolves
only closed local roles, keeps redacted or masked targets safe, and reopens the
boundary when the human explicitly requests the raw or actual value. A pronoun
referring only to metadata does not become disclosure authority.

Mandatory validation, preview, and approval remain harness-owned. Closed
restatements that these controls stay enforced are removed from unmapped
requirements. A bypass, live mutation, or substantive behavior that merely
mentions a control is not consumed by this reconciliation and fails through the
normal adjudication boundary.

### Runtime requirements

The model-facing V4 wire has no authoritative runtime field. For a grounded
build, deterministic analysis derives only the four closed axes from exact
current-human evidence:

```text
restart_persistent
durable_timer
persistent_economy
event_time_llm
```

Positive, negative, alternative, copied, hypothetical, and UI-copy contexts
are separated. Plain timer or economy behavior does not automatically claim the
stronger infrastructure property. A grounded discussion clears runtime state.
Executable business behavior that depends on a runtime property remains exact
unmapped capability evidence unless another closed semantic field represents
that behavior. This prevents a runtime enum from silently consuming unsupported
state transitions, rewards, schedules, or model decisions.

### Recipe details

The detail grounder selects only the requested `copy`, `naming`, and `controls`
paths and creates an immutable path-only ticket before the second model call.
The second request receives only its fixed prompt, the exact current human turn,
and harness-owned detail state. It does not receive the first call's raw Core
arguments, stale detail values, the full Draft, compiled plan, or simulation
trace.

The exact dynamically generated schema instance is reused for parsing. Root and
leaf shape, unknown fields, duplicate JSON keys, empty required values, and
selected-facet coverage are checked before typed binding. Every accepted scalar
or affix must equal the independently rederived literal for that exact slot in
the current turn. A matching value in another slot or an earlier turn is not
enough. Case, punctuation, emoji, and internal whitespace are preserved; only
CRLF uses the existing LF canonical form. Mismatch fails without retry, repair,
compilation, or Draft mutation.

## Evidence, persistence, and restore

Only accepted human decisions advance the request-evidence hash chain. Initial
build evidence and accepted hub resolution have different typed entries.
Malformed, stale, ambiguous, colliding, or rejected answers remain transcript
history but gain no authoritative evidence. Display-key matching is
Unicode-lowercased and separator aware, and exactly one active option must
match.

The append-only transcript stores raw tool arguments for audit. Serving history
projects at most four recent human envelopes plus only successfully replayed
discussion presentation. Old tool calls, tool results, and stale revisions are
not resent. Every eligible history turn is replayed once before context-fit
trimming; failed or non-discussion results contribute no assistant prose.

Snapshot schema 8 requires the exact V4 prompt, protocol 4, extractor revision
16, normalizer revision 11, component identities, stage bindings, and a complete
transcript-integrity digest even for an Empty stage. V3 Intent snapshots and
pre-release V4 extractor revisions through 15 or normalizer revisions through 10
are rejected. V6 and V7 non-Intent snapshots may be promoted at the CLI edge;
V6 or V7 snapshots containing Intent state are rejected.

Durable admission caps the serialized transcript at 4 MiB, human turns at
1,024, and persisted failure results at 16. Overflow before a model call is
rejected; overflow caused by a response rolls the complete turn state back.
SQLite compare-and-swap applies the same bounds before writing.

These hashes detect corruption and inconsistent partial edits. They are not a
MAC or signature. An attacker able to replace the SQLite snapshot and recompute
all digests is outside the present boundary; a Keychain-backed authenticated
envelope remains future work.

## Active version and serving pins

```text
Intent protocol                    4
Intent adjudicator                 3
Intent workspace schema            2
Intent identity revision           2
Session snapshot                   8
Recipe extractor revision         16
Recipe normalizer revision        11
Recipe compiler revision           1
Recipe simulator revision          1
Recipe version                     1
Capability manifest version        1
SQLite store schema                2
Intent evaluation input schema     3
Intent evaluation report schema    5
requested and accepted model       gemma4:12b-mlx
declared benchmark context          16384 tokens
OpenAI-compatible gateway           http://127.0.0.1:18080/v1
```

The current recipe-registry digest pinned by the evaluator is:

```text
5783590262c2971922aa54d4262b37107489c8dbe88678fb1a60e27e39b8858c
```

The gateway-reported model tag is checked exactly. It is not a weights digest.
The 16,384-token value is a declared benchmark policy because the gateway does
not report its active context window; never describe it as observed gateway
configuration.

## Deterministic verification already established

The V4 Rust suite covers identity sensitivity and convergence, V4 strict Core
shape, human evidence, hub resolution, request-mode and preview grammar, copied
payload isolation, live/secret/gate boundary roles, runtime ownership,
capability reconciliation, slot-specific detail binding, transcript replay,
stage and Draft binding, snapshot incompatibility, transcript limits, atomic
rollback, and V3/V4 equivalent recipe output.

At the final semantic-grounding checkpoint before maintenance splitting:

```text
cargo test -p design-harness --lib --locked --quiet
648 passed; 0 failed

cargo test --workspace --locked --quiet
passed

cargo clippy -p design-harness --all-targets --locked -- -D warnings
passed

cargo fmt --all -- --check
passed

git diff --check
passed
```

Independent counterexample review also drove tests for quote-state locality,
generic and Korean operative conditions, multi-antecedent selection, masked raw
secret values, metadata pronouns, unrelated live sentences, production-resource
roles, first-person deploy language, quote-scan linearity, secret-span
linearity, and standalone automatic approval. These are deterministic tests,
not live-model results.

After maintenance commit `f37cc38`, the complete local deterministic branch
gate was rerun successfully:

```text
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
npm --prefix eval/design-harness run check
git diff --check
dependency-guard and no-Rust-comment scans
credential and changed-scope audit

result: green
```

The DB-less and PostgreSQL GitHub Actions jobs are still pending because the PR
has not been opened. Any later code change must rerun the local gates; the
documentation-only handoff commit must at least remain `git diff --check`
clean.

## Maintenance split status

Maintenance is part of the product quality bar, not optional cleanup. Every
split is behavior-preserving: no public API, serialization, tool name, error
code, digest projection, prompt, or safety behavior may change in a refactor
commit.

Completed commits at handoff drafting:

```text
d401e03 refactor(eval): remove stale normalizer version labels
3e896db refactor(intent): split safety control grammar
8b272a6 refactor(intent): isolate metalinguistic scope
e15e570 refactor(intent): split boundary classification
f37cc38 refactor(intent): split boundary evidence
```

The safety grammar is now a small facade over responsibility modules for core,
English, Korean, lexicon, and tests. Quote/metalinguistic literal scope is a
neutral turn-level module instead of a request-mode-owned helper imported by
safety grounding. Boundary classification is split into vocabulary, action
authority, action polarity, gate control, live scope, secret disclosure, and
unit scope. Boundary evidence is split into canonicalization, grouping, and
coverage. The post-split full local gate is green. Inspect the actual branch
before adding any further refactor and avoid over-decomposition.

## Evaluation contract and evidence boundary

The V4 evaluator contains 26 fixed Intent cases covering English and Korean
defaults; a pure single-turn English paraphrase; independent hub, locale, close,
copy, naming, and control mutations; hub clarification and SQLite reopen;
discussion then build; typed-planner classification; creator-only and
stateful-runtime gaps; gate bypass and safe restatement distinctions; redaction;
exact unknown capability; full custom and copy-only details; live mutation and
secret disclosure; and request-grounding regressions.

Acceptance requires exact model and context policy, clean committed source,
matching source and binary identity, no cache, concurrency one, non-overlapping
runs, exact call counts, zero automatic retry, independent RuleSet path checks,
current validation and simulation, evaluator-recomputed candidate hashes,
identity-class convergence and separation, and latency gates. The provider
records request bytes, message/tool/schema bytes, token usage when reported,
finish reason, HTTP attempt, served-model provenance, client-observed request
duration, and total turn duration.

The required live order is:

1. one clean-source full-case smoke;
2. natural default, custom-full, and copy-only ten times each;
3. hub, locale, close, copy, naming, and control mutations three times each;
4. one-shot, clarification, restart, discussion-build, and paraphrase
   equivalence;
5. final clean-source acceptance summary.

Latency is evaluated separately for one-call and two-call previews. The current
contract keeps one-call P50 below 8 seconds and P95 below 20 seconds, two-call
P50 at or below 22 seconds and P95 at or below 30 seconds, and every turn at or
below 60 seconds.

### What has actually been measured

There is no live V4 Promptfoo result at this checkpoint. V4 live smoke,
repeat-stability, identity-class, and latency results are pending.

The following are historical V3 observations only:

| V3 cohort | Result | Correct interpretation |
|---|---:|---|
| Ten-case baseline, one repetition each | 10/10 | One clean pass, not ten repeats per case |
| Four contrast cases, three repetitions each | 12/12 | Small boundary/capability routing sample |
| Full copy+naming+controls | 3/3 | Stable RuleSet and compiled plan; semantic/input identity had two variants |
| Copy-only reduced frontier | 1/1 | One end-to-end reduced-detail sample |
| Post-refactor default+contrast regression | 14/14 | One clean-source behavior-preservation sample |

Those V3 rows justify why V4 was designed. They do not prove that V4 now passes
the live model cohort. Raw historical reports live only under the ignored local
`eval/design-harness/results/` directory when present and must never be added to
Git.

## Commercial limitations

Even a fully passing V4 local cohort certifies a bounded authoring harness, not
a commercial service. The following remain open:

- only `starring.private_study_room@1` compiles as a product recipe;
- typed-planner fallback is classified but not connected to a typed planning
  session;
- arbitrary multi-turn typed preference accumulation and preview edit/recompile
  with `keep`, `set`, and `reset_default` do not exist;
- discussion presentation is not a durable structured brainstorming model;
- the harness is a CLI and evaluation edge, not an authenticated API or UI;
- manual actor input is not production identity assurance;
- no queueing, concurrent-load, saturation, throughput, tail-latency, soak, or
  recovery SLO has been established for the Mac mini;
- the gateway tag and opaque gateway identity do not prove exact model weights
  or server configuration;
- the 16K context is declared, not observed from the gateway;
- local SQLite integrity is unkeyed and is not attacker-resistant
  authentication;
- no whole-action-plan preflight runs before the first Discord side effect;
- external Discord provisioning lacks persisted compensation, reconciliation,
  and uncertain-effect replay idempotency;
- production publication and activation integration still requires the
  authenticated user-approval boundary;
- stateful games, timers, economies, and sessions require a separate
  `StatefulSpec` and runtime design rather than new ad-hoc recipe fields.

## Exact continuation order

### 1. Commit the documentation checkpoint

Check the branch and dirty tree first. The planned responsibility splits and
post-split full local gates are complete; do not repeat the splits. Review and
commit only `CURRENT_STATE.md` and this handoff as one documentation unit, with
`git diff --check` clean. If any code changes after `f37cc38`, rerun the entire
local gate before live evaluation. The engine, runtime, publication, approval,
activation, Discord, and production-database crates remain out of
semantic-change scope.

### 2. Run the Gemma-only V4 live evaluation

The live provider refuses dirty source and a mismatched binary. Commit the docs
and maintenance first. Retrieve the API key from Keychain or an existing
environment variable without printing it. Keep the gateway, model, context,
cache, and concurrency policy exact.

```sh
export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/opt/homebrew/bin:/usr/bin:/bin:$PATH"
export STARRING_LLM_BASE_URL=http://127.0.0.1:18080/v1
export STARRING_LLM_API_KEY="$(security find-generic-password -s com.starring.llm-api-key -a llm-api -w)"

npm --prefix eval/design-harness run eval:intent -- \
  --repeat 10 \
  --output results/gemma4-intent-v4.json
npm --prefix eval/design-harness run summarize -- results/gemma4-intent-v4.json
npm --prefix eval/design-harness run accept:intent -- results/gemma4-intent-v4.json
```

Use the case slicing and repeat order in the V4 spec when collecting the smoke,
ten-repeat non-mutation, and three-repeat mutation artifacts. Never weaken an
assertion to turn a model failure into a pass. Record each failure point,
identity variance, retry count, finish reason, and one-call/two-call latency
honestly in `eval/design-harness/measurements.md`. Keep raw reports ignored.

### 3. Publish, review, and merge

Push `feat/intent-v4-semantic-identity`, open a PR against current `main`, and
include the semantic boundary, deterministic gates, live V4 results, maintenance
layout, safety scope, and limitations in the PR body. Require the DB-less and
PostgreSQL GitHub Actions jobs to pass. Require an independent diff and safety
review. Merge only after those checks, then fetch and verify that local `main`
contains the merge and is clean.

### 4. Start the next product capability only after merge

The next structural feature is typed multi-turn preference accumulation and
atomic recipe-owned-region edit/recompile. It must define provenance, conflicts,
stale revision behavior, `keep`, `set`, and `reset_default`, and convergence
between equivalent one-shot and multi-turn designs. Do not mix that feature into
the V4 identity certification branch.

After that, implement the typed-planner handoff, production authenticated
authoring edge, commercial load evidence, whole-plan preflight and external
recovery, a second bounded recipe, and only then the separate stateful-runtime
arc.

## Implementation map

- V4 protocol and Core parser:
  `crates/design-harness/src/turn/intent_core.rs`
- Request mode, quote scope, and preview authority:
  `crates/design-harness/src/turn/intent_request_mode_grounding*` and
  `crates/design-harness/src/turn/intent_metalinguistic_scope.rs`
- Safety-boundary grounding:
  `crates/design-harness/src/turn/intent_boundary_grounding*`
- Shared gate grammar and polarity:
  `crates/design-harness/src/turn/intent_safety_control_grammar*`
- Operative conditional and quote scanner:
  `crates/design-harness/src/turn/intent_operative_conditionals.rs` and
  `crates/design-harness/src/turn/intent_quote_scanner.rs`
- Runtime grounding:
  `crates/design-harness/src/turn/intent_runtime_grounding*`
- Capability grounding and harness-owned control reconciliation:
  `crates/design-harness/src/turn/intent_capability_grounding*` and
  `crates/design-harness/src/turn/intent_capability_reconciliation*`
- Detail path ticket, schema, parser, literal binding, and validation:
  `crates/design-harness/src/turn/intent_detail_*` and
  `crates/design-harness/src/turn/intent_recipe_details*`
- Request evidence, serving, execution, restore, replay, and transcript binding:
  `crates/design-harness/src/session/intent_routing/`
- Shared canonical digest helpers:
  `crates/design-harness/src/intent/identity.rs`
- Compiler-input, semantic-intent, plan, and candidate identities:
  `crates/design-harness/src/intent/`
- CLI HTTP metrics and SQLite edge:
  `tools/design-harness/`
- V4 case manifest, assertions, acceptance, and summaries:
  `eval/design-harness/`
- Historical and pending live measurements:
  `eval/design-harness/measurements.md`

## Non-negotiable continuation rules

- Use only `gemma4:12b-mlx` for this acceptance line and declare 16,384 tokens.
- Keep API keys and gateway credentials in environment or Keychain only.
- Never commit files under `eval/design-harness/results/`.
- Keep `design-harness` pure: no SQLite, SQLx, Twilight, HTTP, gateway, or
  platform dependency in the library.
- Do not change engine, event-time runtime, publication, approval, activation,
  deployment, Discord, or production-database safety boundaries while tuning
  Intent.
- Keep model-authored prose non-authoritative for semantics, safety, capability,
  recipe identity, compilation, deployment, and runtime.
- Preserve zero automatic model retries and the one-call/two-call frontier
  contract.
- Keep candidate validation, simulation, preview, and atomic Draft commit
  mandatory.
- Use feature-sized commits and separate behavioral changes from pure moves.
- Preserve the repository's no-Rust-comment rule.
- Report regressions and failed live samples; do not pool V3, V4, Qwen, Ornith,
  dirty-source, or different-concurrency evidence.

## Final checkpoint interpretation

V4 moves the first recipe from “the same executable result happened despite a
prose-dependent identity” to an architecture where human provenance,
authoritative semantics, executable plan, artifact, Draft state, and transcript
bytes have separate deterministic contracts. It also moves request mode,
safety, runtime, and detail ownership out of stochastic model fields and into
bounded human-grounded code.

That is the correct structural foundation, but the commercial claim is not yet
earned. Behavior-preserving maintenance and full clean local gates are complete.
The immediate remaining evidence is a clean-source Gemma V4 cohort that proves
the identity classes and latency contract in practice, followed by PR review,
CI, and main merge. Only then should the project begin typed multi-turn
edit/recompile work.
