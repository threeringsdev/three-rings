# Component Gap Analysis Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Produce `design/component-gap-analysis.md` — a deduped catalog mapping every wireframe UI primitive to the Rust/UI registry with direct/composite/gap verdicts plus a code-level SSR/hydration review of six components — and land the spec findings, open-question resolutions, and TODO follow-ups.

**Architecture:** Paper analysis in four passes: (1) walk every wireframe screen via Pencil MCP into a primitive inventory; (2) map the inventory against the pinned registry into the deliverable catalog; (3) code-review six upstream component sources against a hazard checklist; (4) record findings/annotations/queue updates and flip the checkbox. Approved design: [docs/superpowers/specs/2026-07-10-component-gap-analysis-design.md](../specs/2026-07-10-component-gap-analysis-design.md).

**Tech Stack:** Pencil MCP (read-only), `gh api` against rust-ui/ui, markdown.

## Global Constraints

- **Registry pin:** rust-ui/ui commit `43e1e32` (2026-07-01), components under `app_crates/registry/src/ui/`. Every mapping/review claim cites this commit.
- **Paper only:** no components vendored, no app code, no Cargo/Tailwind changes.
- **Pencil is read-only this task:** batch_get / get_editor_state / get_screenshot only — no batch_design, so no editor save is needed for commits.
- **`.pen` files are encrypted** — never Read/Grep them; Pencil MCP only.
- **Verdict classes (design doc):** Direct / Composite / Gap; app-specific composites (three-counts row, card grid) are feature-side work per ui-components scope, not registry gaps.
- **Decisions already made (maintainer, 2026-07-10):** runtime spot-check skipped → deferred to `specs/ui-component-bench.md` (draft, committed); global ⌘K palette is **v1**.
- **Registry component list at pin (verbatim):** accordion action_bar alert alert_dialog animate aspect_ratio attachment auto_form avatar badge bento_grid bottom_nav breadcrumb bubble button button_action button_group callout card card_carousel carousel charts chat checkbox chips collapsible command context_menu data_grid data_table date_picker date_picker_dual_state date_picker_state dialog direction_provider drag_and_drop drawer dropdown_menu empty expandable faq_transition field footer form header hover_card image input input_group input_otp input_phone input_prompt item kbd label link marker marquee mask menubar message mod multi_select navigation_menu pagination popover pressable progress radio_button radio_button_group scroll_area select select_native separator sheet shimmer sidenav skeleton slider sonner spinner status switch table tabs textarea theme_toggle toggle_group tooltip
- **Queue DoD (specs/README.md):** final work + `[~]`→`[x]` in the same commit; findings in the gating specs; follow-ups as new `[ ]` tasks.

---

### Task 1: Wireframe screen walk → primitive inventory

**Files:**
- Create: `<scratchpad>/gap-inventory.md` (session scratchpad — working file, never committed)

**Interfaces:**
- Produces: inventory table `| primitive | screens | notes |` consumed verbatim by Task 2. "Primitive" = a UI element type a component library would own (field, toast, sheet, stepper…), not app content.

- [ ] **Step 1: Load Pencil schema and enumerate top-level frames**

Load `mcp__pencil__*` schemas via ToolSearch (`select:mcp__pencil__get_editor_state,mcp__pencil__batch_get,mcp__pencil__get_screenshot`), then `get_editor_state(include_schema: true)`. List every top-level frame in `design/wireframes.pen`. Expected set (flag any surprise in the inventory notes): desktop screens (Collection view, Catalog search, Sign in), mobile screens (My cards root, Collection view, Catalog search, Catalog filter sheet, Card sheet), the hover-preview overlay, reusable components (Tree Item `HbhAn`, Card Row `Jz0XP`, Card Tile `Infpe`), and the four storyboard containers `Proto — Add flow · Desktop / Deck context / Catalog / Mobile` (`w3i55`/`oBPnh`/`rdJuu`/`JzdVN`).

- [ ] **Step 2: Walk each frame with batch_get and record primitives**

For each top-level frame, `batch_get` its subtree (depth enough to see leaf controls; screenshots only if structure is ambiguous). Append rows to `gap-inventory.md` as primitives appear; extend the `screens` column when a known primitive recurs. Seed expectations to confirm or correct (from the design doc): collection tree (drag-reparent, badges, collapse), three-counts row header, in-place count stepper, quick-add type-ahead panel, destination picker, needs chip, undo toast, hover preview, card grid tile, filter rail controls, mobile bottom tabs, bottom sheets, kbd hints, selection tray, auth form, mode switch, user menu, search inputs, buttons/badges/separators.

- [ ] **Step 3: Completeness gate**

Verify: every top-level frame from Step 1 appears in at least one inventory row's `screens` column, or is explicitly listed in a "contributes nothing new" note (e.g. a storyboard close-up duplicating M2's primitives). No commit — scratchpad only.

### Task 2: Registry mapping → deliverable catalog

**Files:**
- Create: `design/component-gap-analysis.md`

**Interfaces:**
- Consumes: `<scratchpad>/gap-inventory.md` from Task 1.
- Produces: catalog with per-primitive verdicts; Gap list; the file Task 3 appends to.

- [ ] **Step 1: Rule on `action_bar` for the selection tray**

Fetch and read the source (design doc mandates this before ruling):

```bash
gh api "repos/rust-ui/ui/contents/app_crates/registry/src/ui/action_bar.rs?ref=43e1e32" --jq '.content' | base64 -d
```

Also fetch any adjacent demo (`app_crates/registry/src/demos/demo_action_bar.rs`) if the API surface is unclear. Verdict: Direct/Composite base if it is a docked bar with slots; Gap if it's something else.

- [ ] **Step 2: Write `design/component-gap-analysis.md`**

Header block (follow `design/information-architecture.md` house style):

```markdown
# Component gap analysis vs. Rust/UI

**Deliverable of:** Phase 1b task "Component gap analysis vs. Rust/UI registry" — see [specs/ui-design.md](../specs/ui-design.md), [specs/ui-components.md](../specs/ui-components.md)
**Approved design:** [docs/superpowers/specs/2026-07-10-component-gap-analysis-design.md](../docs/superpowers/specs/2026-07-10-component-gap-analysis-design.md)
**Registry pin:** [rust-ui/ui](https://github.com/rust-ui/ui) @ `43e1e32` (2026-07-01), components under `app_crates/registry/src/ui/`
```

Then: a short "How to read this" paragraph (verdict classes, one sentence each); the **catalog table** `| Primitive | Screens | Registry match | Verdict |`, one row per inventory primitive, dedup preserved; a **Gaps** section giving each Gap primitive a paragraph (what it needs, why nothing in the registry covers it, nearest registry parts to build on); a **Feature-side composites** note (three-counts row, card grid — excluded per ui-components scope); an empty `## SSR/hydration code review` heading for Task 3 to fill in the same working session (committed only after Task 3 completes — no placeholder lands in git).

- [ ] **Step 3: Coverage gate**

Cross-check: every `gap-inventory.md` row appears in the catalog; every registry name cited exists in the Global Constraints list verbatim; every Gap has a Gaps-section paragraph. Fix misses now.

### Task 3: SSR/hydration code review (6 components)

**Files:**
- Modify: `design/component-gap-analysis.md` (fill the review section)
- Create: `<scratchpad>/registry-src/*.rs` (fetched sources, never committed)

**Interfaces:**
- Consumes: the six names fixed by the design doc: `dialog`, `popover`, `command`, `hover_card`, `sheet`, `sonner`.

- [ ] **Step 1: Fetch the six sources at the pin**

```bash
cd <scratchpad> && mkdir -p registry-src && for c in dialog popover command hover_card sheet sonner; do
  gh api "repos/rust-ui/ui/contents/app_crates/registry/src/ui/$c.rs?ref=43e1e32" --jq '.content' | base64 -d > "registry-src/$c.rs"
done && wc -l registry-src/*.rs
```

Expected: six non-empty files. If any 404s (name moved), list the directory at the pin and adjust:
`gh api "repos/rust-ui/ui/contents/app_crates/registry/src/ui?ref=43e1e32" --jq '.[].name'`

- [ ] **Step 2: Review each against the hazard checklist**

Read each file. Per component, check: client-only APIs at render time (`window`/`document` outside effects); portal usage; effect-driven initial rendering (server/client first-paint divergence = hydration mismatch); `leptos_ui` dependency (nightly hazard — needs the vendored-`clx.rs` treatment like `table` got); non-deterministic IDs; CSR-assuming event wiring. Also fetch any local `super::` module a file imports (same `gh api` pattern) — hazards hide in shared helpers.

- [ ] **Step 3: Write the review section**

Fill `## SSR/hydration code review` with: the checklist (one line), then a table `| Component | Verdict | Evidence |` where Verdict ∈ **adopt as-is / adopt with deviations (named) / needs runtime verification before adoption**, and Evidence cites concrete constructs (function/line references into the pinned source). Close with one line: runtime verification of all six lands with the component bench (`specs/ui-component-bench.md`, draft).

- [ ] **Step 4: Evidence gate, then commit**

Verify each of the six has a verdict with cited evidence (no bare assertions). Then:

```bash
git add design/component-gap-analysis.md
git commit -m "docs(design): component gap analysis — primitive catalog + SSR code review vs rust-ui@43e1e32"
```

### Task 4: Findings, annotations, queue updates, checkbox

**Files:**
- Modify: `specs/ui-design.md` (Findings entry; palette open question)
- Modify: `specs/ui-components.md` (open question annotation; Design note)
- Modify: `specs/TODO.md` (two new tasks; `[~]`→`[x]`)

- [ ] **Step 1: ui-design.md**

Add a Findings entry (top of Findings list, house style) — exact frame, result slots «» filled from the committed deliverable:

```markdown
- 2026-07-10 — **Component gap analysis complete** (Phase 1b task 4); deliverable: [design/component-gap-analysis.md](../design/component-gap-analysis.md) — «N» primitives cataloged against rust-ui/ui `43e1e32`; «D» direct, «C» composite, «G» gaps («gap names»). Maintainer decisions: the global ⌘K command palette is **v1** (the registry's `command` component carries the palette UI and the quick-add panel alike — follow-up design task queued); the runtime SSR spot-check was replaced by a standing component-bench page ([ui-component-bench](ui-component-bench.md), draft).
```

Change the palette open question line to:

```markdown
- Keyboard-driven command palette for power users — v1 or later? *(resolved 2026-07-10 — v1, maintainer decision at gap-analysis design review; see Findings and the Phase 1b palette design task)*
```

- [ ] **Step 2: ui-components.md**

Annotate the spot-check open question (replace the existing annotation on that line):

```markdown
- *(resolved 2026-07-10 — spike + gap analysis: `table` verified at runtime in the spike; dialog/popover/command/hover_card/sheet/sonner code-reviewed against rust-ui `43e1e32` in [design/component-gap-analysis.md](../design/component-gap-analysis.md); runtime verification of all six deferred to [ui-component-bench](ui-component-bench.md))* Rust/UI is young (~300 stars) — spot-check the components we need (dialog, popover, table, combobox) for SSR/hydration correctness before committing broadly.
```

Append one bullet to the Design section list:

```markdown
- *Gap-analysis note (2026-07-10):* the registry has no combobox — `command` (+ `popover`/`input`) is the stand-in for every type-ahead surface (quick-add panel, destination picker, ⌘K palette). Registry gaps needing custom components are cataloged in [design/component-gap-analysis.md](../design/component-gap-analysis.md).
```

- [ ] **Step 3: TODO.md — follow-ups + flip**

Insert after the gap-analysis task line, before the (now `[x]`) hygiene line:

```markdown
- [ ] Design the global ⌘K command palette (actions, scope, wireframe) (specs: [ui-design](ui-design.md))
```

Insert at the top of Phase 3:

```markdown
- [ ] Build the component bench page — every vendored component with variants, one route (specs: [ui-component-bench](ui-component-bench.md))
```

Flip the task line to (result slot «» from the deliverable):

```markdown
- [x] Component gap analysis vs. Rust/UI registry (specs: [ui-design](ui-design.md), [ui-components](ui-components.md)) — design/component-gap-analysis.md: «N» primitives vs rust-ui@43e1e32, «G» gaps + SSR code review; see spec Findings
```

- [ ] **Step 4: DoD gate, then final commit**

Check against specs/README.md DoD: work committed ✓ (Task 3), findings recorded ✓ (Steps 1–2), follow-ups queued ✓ (Step 3), checkbox in same commit as final work ✓ (this commit). Then:

```bash
git add specs/ui-design.md specs/ui-components.md specs/TODO.md
git commit -m "docs(specs): gap-analysis findings + open-question resolutions; queue palette + bench tasks; task done"
```
