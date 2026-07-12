# Phase 18e: Durable RuleSet Rollback — Live Certification Runbook

Executed 2026-07-12. Reused bot, guild `1524810437118525551`, study_hub channel
`1524810437667852431`, local PostgreSQL `starring`. Operator Discord id
`1056857223529250906`. Tool: `interaction-smoke` at commit `4830f1d`.

**Outcome: certified.** After activating RuleSet v2 and rolling back to v1, each
`AutomationInstance` kept dispatching with the immutable RuleSet version pinned at
its creation, a newly created instance pinned the current active version, and the
active pointer, RuleSet artifacts, instance pins, and panel installation state
were all restored from PostgreSQL across two process restarts.

## Isolation key and precondition

Dedicated key `studyroom_18e_20260712_a`. Precondition confirmed empty before
starting:

```
automation_ruleset_versions      count = 0
automation_ruleset_heads         count = 0
automation_ruleset_activations   count = 0
```

Fixture variant vs registry version were recorded from actual publish output, not
assumed:

```
variant V1  ->  registry version 1  (content_hash 1f59ef11f0849dbf)
variant V2  ->  registry version 2  (content_hash e64cd6406532676d)
```

Isolation check: the 18e v1 hash `1f59ef11...` differs from the pre-existing
`studyroom_demo` v1 hash `a1afef40...`, confirming the isolated key and its own
content, and version numbering started cleanly at 1.

## Stage 1 — v1 bootstrap

```
seed-studyroom --variant v1     -> seed: published 1
activate 1                      -> activated 1 (8 notices)     active pointer = 1
run                             -> hydrated ... v1 (8 notices);
                                   panel reconcile [study_panel: Posted];
                                   starting gateway
```

Panel installation record after first run:

```
study_panel  installed_version=1  channel=1524810437667852431
             spec_hash=ecd1df94ac8a  message_id=1525721355012673676
```

Operator created room **R1** (make button on the "스터디룸 만들기 · v1" panel ->
modal -> submit) and clicked its hub join button.

```
R1  instance_id = i_xzwjkmy1y3vv   pin (ruleset_version) = 1   status = active
    roles     { member_role: 1525721773541167214 }
    channels  { room_channel: 1525721775042723881 }
    messages  { welcome_panel: {channel: room_channel, id: ...},
                hub_panel:     {channel: 1524810437667852431, id: ...} }
    join click response: "스터디룸에 참가했습니다. [v1]"
```

Complete four-resource footprint (18d-2) captured at registration, messages in
the `{channel, id}` form (18d-3 model).

## Stage 2 — restart #1 + v2 activation

Gateway stopped. Then:

```
seed-studyroom --variant v2     -> seed: published 2          (registry v1 + v2, next=3)
activate 2                      -> activated 2 (8 notices)     active pointer = 2
                                   R1 pin still = 1 (activation moved the pointer only)
run  (restart #1)               -> hydrated ... v2 (8 notices);
                                   panel reconcile [study_panel: Edited];
                                   starting gateway
```

Panel record after restart #1 (edited in place — same message):

```
study_panel  installed_version 1 -> 2   spec_hash ecd1df94ac8a -> 517416ecb839
             message_id 1525721355012673676 (unchanged)
```

Operator created room **R2** and exercised both join buttons:

```
R2  instance_id = i_f0wt3wj4rrbq   pin = 2   status = active
    R2 join click response: "스터디룸 참가가 완료되었습니다. [v2]"
    R1 join click response: "스터디룸에 참가했습니다. [v1]"   (active is v2; R1 still dispatches pinned v1)
```

Core proof: one running gateway (active v2) served R1 with pinned v1 and R2 with
pinned v2 simultaneously. Restart #1 restored active v2 and preserved R1's pin
from PostgreSQL.

## Stage 3 — restart #2 + v1 rollback

Gateway stopped. Then:

```
activate 1                      -> activated 1 (8 notices)     active pointer = 1
                                   R1 pin=1, R2 pin=2 unchanged;
                                   registry still {v1, v2}, next=3 (rollback re-activated
                                   the existing immutable v1 artifact, no re-seed)
run  (restart #2)               -> hydrated ... v1 (8 notices);
                                   panel reconcile [study_panel: Edited];
                                   starting gateway
```

Panel record after rollback (edited back — same message, spec_hash returned to
exactly the v1 value):

```
study_panel  installed_version 2 -> 1   spec_hash 517416ecb839 -> ecd1df94ac8a
             message_id 1525721355012673676 (unchanged across all three edits:
             Posted v1 -> Edited v2 -> Edited v1)
```

Operator created room **R3** and exercised both join buttons:

```
R3  instance_id = i_9ht98f2dcrrv   pin = 1   status = active   room_channel = 1525723198782443620
    R3 join click response: "스터디룸에 참가했습니다. [v1]"   (new instance after rollback pins v1)
    R2 join click response: "스터디룸 참가가 완료되었습니다. [v2]"   (R2 still pinned v2)
```

Restart #2 restored active v1 and preserved both R1's and R2's pins from
PostgreSQL.

## Stage 4 — teardown regression

Operator clicked "방 닫기" on R3's welcome panel. The room channel was deleted
immediately (teardown order: messages -> channels -> roles). The ephemeral
confirmation was not observed because its channel context vanished, which is the
expected 18d-3 response-separation case (teardown succeeds independently of
response delivery).

```
R3  i_9ht98f2dcrrv   status active -> deleted
    close interaction 1525723732302106755 -> Executed
```

## Optional gated-activation-failure

Not performed live (a live bot-capability revoke would destabilize the reused
guild). The active-pointer-protection property is already certified by 18c-4 live
evidence: activating a not-ready target returns `ActivationError::NotReady` and
leaves the active pointer and published artifact unchanged.

## Final state

```
registry versions:  1 (1f59ef11f0849dbf), 2 (e64cd6406532676d)   next=3
active pointer:      1
panel:               study_panel  installed_version=1  spec_hash=ecd1df94ac8a

instances:
  R1  i_xzwjkmy1y3vv  pin=1  active
  R2  i_f0wt3wj4rrbq  pin=2  active
  R3  i_9ht98f2dcrrv  pin=1  deleted
```

Matches the predicted certification outcome exactly: `active=1, R1=v1, R2=v2,
R3=v1`.

## Known limitations / notes

- One guild, one reused bot; multi-guild concurrency not exercised.
- Active version takes effect for the `make` button and the declared panel only
  after a gateway restart (hydration is once-at-boot; the durable model, which
  the restart steps prove).
- The declared panel was edited in place across all version transitions (single
  per-key panel message).
- R1 and R2 remained active at the end (their Discord role/channel/panels persist)
  pending optional cleanup via their "방 닫기" buttons.
