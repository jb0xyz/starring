# Authoring milestone A6 vertical-slice evidence

Date: 2026-07-30 KST

Status: Phase A milestone evidence, not commercial runtime certification

## Source and host

- live source revision: `418c6386d72c7c0669c0d09d6f3351a161cd19b1`
- model provider: `codex_chatgpt`
- model: `gpt-5.6-luna`
- reasoning effort: `medium`
- authentication mode: ChatGPT
- Codex CLI: `codex-cli 0.146.0-alpha.3.1`
- worker Node: `v24.18.0`
- Rust: Cargo `1.97.0`
- PostgreSQL: `16.14`
- host: `Mac16,10`, 10 physical CPU cores, 25,769,803,776 bytes memory
- macOS: `26.5.2` build `25F84`

The live API release binary and loopback worker both used the source revision
above. The browser runner was a temporary no-store staging surface protected by
Cloudflare Access and product Discord OAuth. It was not a product frontend and
is removed after this evidence is sealed.

## Incident and recovery

The first authenticated attempt returned HTTP 503 before creating an authoring
session. The worker recorded `codex_identity_changed`: the ChatGPT application
had updated its bundled Codex CLI after the long-running worker had pinned
`codex-cli 0.144.2`.

The exact current CLI was observed as `codex-cli 0.146.0-alpha.3.1`, with
ChatGPT authentication still active. Current protocol contracts, tests,
evaluation manifests, and the operations runbook were advanced to that exact
version in revision `418c638`. Worker tests passed 38/38, worker SLO tests passed
70/70, design-harness evaluation tests passed 106/106, the strict worker-client
tests passed 14/14 across its unit and dependency-guard targets, and the
authoring HTTP integration target passed 5/5. The worker and API were restarted,
both reported ready, and one authenticated Luna smoke call passed before the
browser retry.

The retry reused the same fixed idempotency identities. No session, generation,
promotion, receipt, audit event, activation, approval, Apply record, deployment,
or active pointer existed from the failed attempt.

## Live method

One authenticated browser flow performed these ordered steps:

1. Submit a fully specified private study-room request.
2. Require generation 1 to be `preview_ready`.
3. Read that stored session through the authenticated GET endpoint and require
   observed generation 1 to remain `preview_ready`.
4. Submit an underspecified private study-room request.
5. Require its generation 1 to be `needs_input`.
6. Supply the missing existing-channel choice.
7. Require generation 2 to be `preview_ready`.
8. Promote the exact stored one-shot generation 1 through the existing product
   endpoint.
9. Require the product response to be `pending_approval`.
10. Stop without approval or Apply.

The later multi-turn writes and promotion could run only after the authenticated
GET returned the exact stored one-shot generation, so the completed sequence
also proves the live decrypt-and-read composition.

## Model observations

The restarted worker admitted four calls and settled all four. One was the
pre-run smoke call. The A6 flow used three Luna calls with no failure or retry:
two `interpret_intent_core` calls and one `resolve_intent_decision` call.

| Metric | A6 value |
| --- | ---: |
| model calls | 3 |
| successful model calls | 3 |
| failed model calls | 0 |
| input tokens | 22,328 |
| cached input tokens | 4,864 |
| output tokens | 370 |
| reasoning output tokens | 232 |
| summed worker duration | 15,823 ms |
| maximum worker-call duration | 6,842 ms |
| first generation to exact promotion | 17,880 ms |
| first to last generation | 17,011 ms |

The original authentication-to-promotion interval was 443,771 ms because it
includes diagnosis, the exact-version code change, rebuild, and service
restart. It is incident-recovery evidence, not a steady-state latency sample.

## Durable database evidence

Only aggregate counts, lengths, stages, revisions, and equality predicates were
observed. Plaintext snapshots, transcripts, identifiers, key identifiers,
digests, nonces, ciphertexts, and key material were not copied into evidence.

| Contract | Observed result |
| --- | --- |
| sessions | 2 active, both owned by the authenticated principal in the expected tenant and installation |
| one-shot head | generation 1 |
| multi-turn head | generation 2 |
| generation stages | one-shot 1 `preview_ready`; multi-turn 1 `needs_input`; multi-turn 2 `preview_ready` |
| encryption | snapshot schema 8, `xchacha20_poly1305`, suite version 1, 24-byte nonce |
| ciphertext integrity | 3 nonempty ciphertexts, 3 distinct nonces, 3 distinct ciphertexts |
| authenticated metadata | valid on all 3 generations |
| trusted writer metadata | complete on all 3 generations |
| safe projections | 3 nonempty, digest-valid, pairwise-distinct projections |
| authority binding | all 3 generations use immutable authority revision 2 and exactly match its bindings and fingerprint |
| exact promotion | 1 promotion, revision 3, `activation_pending`, bound to the one-shot generation 1 candidate |
| product activation | 1 linked `product_authoring` activation, product revision 1, state `pending`, apply attempt 0 |
| promotion evidence | 1 receipt, 1 audit event, and 1 cross-linked receipt/audit evidence row |
| promotion action | `promotion.promote`, result `promotion_created`, resulting state `activation_pending` |

## Stop boundary

After promotion:

- approval rows: 0
- total product action receipts: 1, the promotion receipt above
- total product audit events: 1, the promotion audit above
- total receipt/audit evidence rows: 1, the promotion evidence above
- runtime deployments: 0
- active RuleSet pointers: 0

No approval, Apply, deployment, active-pointer mutation, Discord mutation, or
event-time model call was performed. Phase B runtime serving and commercial
certification remain incomplete.

## Supporting gates

- full locked Rust workspace build and test commands: green after the live run
- full locked workspace Clippy with warnings denied: green after the live run
- formatting and diff checks: green
- complete `authoring-application-postgres` ignored PostgreSQL 16 suite:
  181 passing tests
- dedicated trusted-writer PostgreSQL 16 target: 10/10
- staging writer lifecycle with 60 privilege-drift mutations: green
- actual staging incremental writer contract: green
- local worker commands: 38/38
- local worker SLO commands: 70/70
- local design-harness evaluation and Promptfoo configuration commands:
  106/106 and both configurations valid
- design-harness dependency audit at the configured high-severity threshold:
  zero exit, with 11 moderate transitive findings retained for follow-up

The local default Node executable was `v26.5.0`; the production worker used the
configured Node `v24.18.0`, matching the GitHub workflow major version. GitHub
CI remains the exact clean-checkout environment.
