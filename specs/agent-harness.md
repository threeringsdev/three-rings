# Remote agent harness ("the agent box")

**Status:** draft
**Depends on:** [delivery-pipeline](delivery-pipeline.md)

## Problem

Phase 2 made the repo the complete contract and the *results* remotely checkable
(PR checks, rolling release, deployed web URL). But *starting* work still
requires the laptop: opening the devcontainer, handing a prompt to an agent,
answering its questions. The missing piece is the sending end — something that
mints a cloud VM from the repo's canonical image, hands a prompt to an agent,
and lets the human trigger, answer, steer, and review **from a phone or any
other machine**, so work continues while detached from the laptop.

This harness is also the chosen vehicle for the Phase 2 loop-proof capstone:
the maintainer decided (2026-07-11) to prove the "fresh container → task → PR →
auto-merge → verify remotely" loop *through the harness* rather than build an
interim GitHub-Actions prompt-runner (approach B below).

## Scope

**In:**

- One **wake-on-demand agent box** — a Fly Machine running the repo's
  devcontainer image plus the agent tooling — with a persistent volume (repo
  clones, cargo caches, pairing state).
- **Phone control plane via [Happy](https://happy.engineering/)** (OSS, MIT —
  [slopus/happy](https://github.com/slopus/happy)): spawn sessions, answer
  agent questions, steer, receive push notifications. Hosted relay initially
  (E2EE — it routes only encrypted blobs).
- **Waker** (start a stopped box from anywhere) and **idle-stop** (box stops
  itself when no sessions are active).
- **Bootstrap runbook** — pairing, secrets, box creation — good enough to
  rebuild the box from scratch with no other information.
- A **harness repo** (own project, personal account, repo-agnostic by design);
  three-rings is the proving ground. This spec is the design of record until
  that repo self-hosts its docs.
- The **three-rings loop-proof capstone** executed through the harness.

**Out (explicit non-goals for v1):**

- A custom gateway (Telegram bot + `ask_human` MCP tool) — that is the
  documented **fallback** design if Happy disappoints, not the v1 build.
- GitHub-trigger surface (a `workflow_dispatch` prompt-runner) — deliberate,
  per approach B; can be added later as a thin add-on.
- Scheduled / autonomous queue-working (cron picks the next TODO task) —
  parked; the design doesn't preclude it (cron + Machines API + canned prompt).
- Multi-box fleets and concurrent-session worktree choreography — one person,
  and the TODO queue serializes work anyway.
- Self-hosted Happy relay — a hardening milestone, not v1 (E2EE makes the
  hosted relay acceptable meanwhile: it sees only encrypted blobs).
- Fly **Sprites** — right product layer (native idle/checkpoint) but they boot
  full Linux VMs rather than arbitrary Docker images, and plans reportedly
  floor at ~$20/mo; re-evaluate at hardening.

## Design

### Decision record (2026-07-11)

- **Requirements gathered:** triggers = phone + CLI-from-any-machine +
  self-managed cloud (explicitly *not* Anthropic-hosted VMs); interaction =
  fire-and-forget default, **mobile-friendly escape hatch** (explicitly not
  PR/issue comments), rare live steering; billing = **Max subscription** OAuth
  token (`claude setup-token`) so spawned runs have no marginal cost; harness =
  own repo-agnostic project, three-rings first.
- **Evaluated:** Fly Sprites (see non-goals), Electric Agents (a TypeScript
  framework for *authoring* durable agents — wrong layer; we run Claude Code
  against a repo), Orkes/Conductor (workflow orchestration — no compute, no
  agent; wrong layer), GitHub-Actions interim runner (rejected by maintainer —
  no interim system), **Happy (chosen)**.
- **Why Happy:** its daemon (`happy daemon`) registers a machine over an
  outbound WebSocket and exposes a `spawn-happy-session` RPC — **the phone app
  can start new sessions on the box, in a chosen directory, with a prompt**
  (read from source: `packages/happy-cli/src/daemon/`; not yet in their docs).
  Escape hatch (push notification → answer in chat), live steering, permission
  prompts, and a mobile-friendly web app all come with it. E2EE relay,
  self-hostable (~1.3k-line TS server + Postgres + Redis).
- **Happy risk, accepted with mitigations:** young project (b. 2025-07), small
  team, docs lag code. Mitigations: the box never *depends* on Happy —
  `fly ssh console` + bare `claude` always works; an active community fork
  (happier) exists; the Telegram+MCP fallback gateway design is recorded here.

### Architecture

```
you (phone / laptop / anywhere)
  │  wake (iOS Shortcut / script → Fly Machines API)   ← only when box is asleep
  │  spawn / steer / answer (Happy app or web, E2EE)
  ▼
Happy relay (hosted first; self-hostable later — routes encrypted blobs only)
  ▼
agent box (Fly Machine, wake-on-demand)
  ├─ image:   dgoings/three-rings + node + happy CLI + claude CLI
  ├─ volume:  repo clones · cargo caches (warm builds) · happy pairing state
  ├─ secrets: CLAUDE_CODE_OAUTH_TOKEN · GH_TOKEN · DATABASE_URL (Neon dev)
  ├─ happy daemon → registers box in the app; phone spawns sessions w/ prompts
  └─ idle-stop    → no active sessions for ~30 min → stops itself (≈$0 asleep)
  ▼
repo contract takes over: branch → verify suite → PR → auto-merge → Render/APK
```

**Persistent-but-sleeping beats fresh-VM-per-task** because of cargo caches: a
fresh VM pays a cold Rust release build (10+ min) every task; the volume makes
the second task fast. "Fresh" is still proven — once, deliberately, in the
capstone milestone.

### Components

- **Harness repo** — `Dockerfile` (agent layer atop a repo's devcontainer
  image), `fly.toml`, idle-stop script, waker instructions, bootstrap runbook.
  The harness repo is itself the contract, same philosophy as three-rings.
- **Waker** — `happy daemon` dials *out*, so no inbound request can auto-start
  a stopped Machine. v1: an iOS Shortcut (or any script) calling the Fly
  Machines `start` endpoint; `fly m start` covers the CLI case. A CF-Worker
  button page is an optional nicety, not v1.
- **Idle-stop** — in-box timer: no live Happy sessions for ~30 min → calls the
  Machines API to stop its own machine. (The fiddly bit Sprites would provide
  natively.)
- **Auth on the box** — `claude setup-token` → `CLAUDE_CODE_OAUTH_TOKEN` (Max
  plan) and a fine-grained `GH_TOKEN`, both as Fly secrets; Happy pairing done
  once at bootstrap (SSH in, or pair on the laptop and copy the credential
  files to the volume — resolve exact UX in the spike). No secret ever enters
  the image or git.

### Flows

- **Fire-and-forget (default):** wake if needed → Happy: new session in
  `~/three-rings`, type or dictate the prompt → agent follows CLAUDE.md
  (branch, verify, PR, arm auto-merge) → push notification → review the
  PR/Render/APK from anywhere.
- **Escape hatch:** agent hits ambiguity → asks in-session → push notification
  → answer in the app chat. No PR-comment ping-pong.
- **Live steer (rare):** open the session in the app and drive; worst case
  `fly ssh console` and run `claude` bare.

### Failure modes

Relay down / Happy stalls → SSH + bare `claude`; fallback gateway design
recorded; community fork exists. Box wedged → restart from Fly's dashboard
(works on mobile). Runaway cost → idle-stop + Fly spend alerts. Quota
exhaustion → sessions fail politely against the Max plan; no surprise bills.

### Cost

Compute only while working (performance-class Machine ≈ $0.09/hr → heavy month
≈ $5); volume 20–40 GB ≈ $3–6/mo; asleep ≈ pennies; hosted relay $0.
**≈ $5–12/mo.**

### Milestones

1. **Laptop spike (~30 min, zero commitment):** run `happy` in place of
   `claude` once; try `happy daemon` on the Mac; verify pairing, push
   notifications, spawn-from-phone, and subscription-token passthrough.
   Kills the design cheaply if the UX disappoints.
2. **Agent box v1:** harness repo, image, volume, secrets, daemon, idle-stop,
   waker → one real three-rings task completed phone-only.
3. **Loop-proof capstone (closes three-rings Phase 2):** rebuild the box *from
   scratch* following only the runbook — the "fresh container from repo +
   documented secrets" proof — then run a trivial visible task (home-page
   subtitle change) from the phone: PR → auto-merge → verify Render + APK away
   from the laptop. Record in [delivery-pipeline](delivery-pipeline.md), mark
   it implemented (the `.dmg` dispatch remains its own pending item).
4. **Hardening (parked):** self-hosted relay (verify push notifications
   survive), CF-Worker waker, GitHub-trigger add-on, Sprites re-evaluation.

## Success criteria

- [ ] Spike: pairing, push notification, and spawn-from-phone verified on the laptop
- [ ] Box wakes from the phone in under a minute; session spawned from the phone in `~/three-rings`
- [ ] One real three-rings task done phone-only: prompt → PR → auto-merge → verified remotely
- [ ] Fresh-box rebuild from the runbook alone (documents-are-sufficient proof)
- [ ] Loop-proof capstone recorded in delivery-pipeline.md; its Phase 2 task closed
- [ ] Box stops itself when idle; monthly cost ≤ ~$12

## Open questions

- Happy pairing on a headless box: QR over `fly ssh console`, or pair on the
  laptop and copy credential files to the volume? (Resolve in the spike.)
- Does `CLAUDE_CODE_OAUTH_TOKEN` pass through the `happy` wrapper cleanly?
  (Resolve in the spike.)
- Push-notification delivery with a **self-hosted** relay (likely routes via
  Expo's push service) — only matters at hardening.
- Harness repo name and home (personal account assumed).
- Sprites: arbitrary-image support and plan floor — re-check at hardening.
