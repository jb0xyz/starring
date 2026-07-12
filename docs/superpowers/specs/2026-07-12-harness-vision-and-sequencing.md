# Stage 2 — Harness Vision and Sequencing

This is a direction document, not a buildable spec. It records the resolved
product vision for the AI harness and the sequencing decision that scopes the
first buildable slice. The concrete first-slice spec comes from a separate
brainstorm.

## The vision (resolved)

Not merely a "Discord automation designer" — a **conversational Discord app /
game builder**: describe what you want in a conversation with an AI, and it
builds it, where "what you want" can be almost anything you can do inside
Discord, including stateful behavior.

Concretely, capabilities span a spectrum of increasing behavioral depth:

```
①  welcome message · role buttons · channel/role creation · study rooms        (current engine)
②  select-menu roles · slash commands · forum auto-tag · scheduled-event pings   (surface breadth,
   · embed styling · thread creation · automod rules · onboarding screens         still deterministic)
── everything above is "decided at design time, executed deterministically at event time" ──
③  XP/leveling · coin economy · "3 warnings → ban" · "auto-delete after 5 min"   (behavioral depth:
   · quiz games · multi-step onboarding · "open a channel when 10 people join"     state/conditions/timers/sessions)
```

**The chosen vision is ③** — the full-depth builder.

## ③ is an arc, not a phase

③ requires a **stateful runtime** (state variables, conditions, timers,
sessions) that the current engine deliberately excluded (the `conditions`
rejection test is the evidence). This is a multi-phase arc, comparable to what
16–18 was for the interaction plane — a new execution paradigm alongside the
existing one, not a small extension.

## ③ is compatible with the safety model — nothing built is wasted

A stateful runtime is still **deterministic at execution time**. State, conditions,
timers, sessions, games, and economies are all deterministic given the design and
the inputs; no LLM runs at event time. They are **designed by the AI at authoring
time and executed deterministically at event time** — exactly the invariant the
engine already enforces.

Therefore ③ **extends** the engine; it does not discard it:

- Event-time LLM stays forbidden.
- The 18f approval-bound activation boundary, durable versioning, and pinned
  dispatch remain the substrate.
- ③ adds a deliberate new construct (a `StatefulSpec` / session + state runtime)
  **alongside** the RuleSet engine, sharing the durability/approval/pinning layer.

**One caveat:** a genuinely runtime-AI feature — an LLM that chats with users in
real time — would break the event-time-LLM-forbidden invariant. That is a
separate, conscious decision, not part of ③'s deterministic stateful logic.

**One guardrail:** build ③ as a deliberate new construct, not by creep-generalizing
`RuleSet` into an ad-hoc workflow engine ("just add one counter…"). A conscious
stateful spec layer keeps the clean deterministic model intact.

## Two orthogonal tracks

```
Track H (harness)          the conversational designer — produces specs
Track R (stateful runtime) executes ③'s state / conditions / timers / sessions
```

③ needs both eventually. The question is order.

## Sequencing decision: harness first, on the ②-engine

Even with ③ as the goal, the first slice is **Track H (a harness MVP) on the
current engine**:

- The **riskiest unknown** is "can an E2B / gemma-class LLM actually drive a
  multi-step conversational design through tools?" The eval only proved simple
  tool-calling, not an 8-step design loop. **If this fails, ③ is moot** — no point
  building a large stateful runtime if the conversational front end doesn't work.
- This unknown is **validatable now, cheaply**, on the current engine (reconstruct
  the study room through conversation), with no new runtime.
- The stateful runtime (Track R) is known-hard but not uncertain — a large build
  best done *after* the harness proves out.
- Doing both at once repeats the "two half-done things" trap that stage 1
  deliberately avoided.
- The harness's Draft and tool model should be designed **③-extensible**, so that
  adding state/timer/condition tools later does not require rearchitecting it.

So: prove "can it design conversationally?" first → then build the stateful
runtime as a deliberate arc → the harness grows into it. And ③ itself needs a
**concrete first target** (e.g., leveling + economy, or one specific game), not
literally "everything" at once.

## The first buildable slice

A harness MVP: a conversational designer that builds a **Draft** ruleset through
tool use, **never touches live Discord**, and lands only through the existing
gates (validate → policy → preview → approval-bound activation). Its tool
registry has **no `activate`/`deploy`** — the 18f boundary is the safe substrate.
The concrete shape (tools, Draft model, loop, degradation, the target model) is
the subject of the next brainstorm.

## Relationship to prior notes

Supersedes the "undecided product fork (automation vs game)" framing in the
harness-direction memory: the fork is resolved to ③, sequenced harness-first.
The safety architecture (`agent-harness` + `design-tools` + `design-draft`, tools
build Drafts only, no deploy tool) from that note still stands and is the input
to the first-slice brainstorm.
