# Add-Flow Prototype Storyboards Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the add-to-collection click-through storyboards in `design/wireframes.pen` per the approved design in [design/add-flow-prototype.md](../../../design/add-flow-prototype.md), producing the input-cost accounting that validates time-to-enter-50-cards.

**Architecture:** Four clearly-named storyboard container frames (`Proto — Add flow · Desktop / · Deck context / · Catalog / · Mobile`) are added to the existing wireframes document, each a horizontal row of captioned state cells. Close-up states are deep copies of the existing Quick Add panel with per-state descendant overrides; milestone states are copies/adjustments of existing full screens. Accounting notes make each storyboard a keystroke ledger.

**Tech Stack:** Pencil MCP tools (`batch_get`, `batch_design`, `snapshot_layout`, `get_screenshot`) against `design/wireframes.pen`; git for checkpoints; Markdown edits to `specs/ui-design.md` and `specs/TODO.md`.

## Global Constraints

- `.pen` files are encrypted — access ONLY via pencil MCP tools; NEVER `Read`/`Grep`/`Edit` on `design/wireframes.pen`.
- The file is open in the maintainer's editor (multiplayer). **At the start of every task, re-verify the node IDs you are about to touch with `batch_get`** — the ID table below is a snapshot, not a guarantee. If an ID is gone, search by node name.
- Every new/copied/modified **root** frame carries `placeholder: true` while being worked on; unset it in the same task the frame is finished in.
- Every created node gets a human-readable `name`. No comments inside `batch_design` JS.
- Use existing document variables — `$font`, `$bg`, `$surface`, `$text`, `$text-2`, `$text-3`, `$border`, `$hover`, `$selected`, `$accent` — never hardcode new colors except `#FFFFFF` (already used by the panel) .
- Root-level placement only via `FindEmptySpace`; never overlap root nodes.
- Verification discipline (the "test" for every task): `snapshot_layout({filePath, parentId: <container>, problemsOnly: true})` must return no problems, and each storyboard row gets exactly ONE `get_screenshot` when it is complete — not per batch.
- When copying `KMq8b` (Quick Add Open), state overrides go in the `Copy(...)` call's `descendants` map keyed by ORIGINAL child IDs — never separate `Update`s on copied children (IDs change on copy).
- Wireframe fidelity: match existing style (13px row text, 11px meta/hints, 10px section labels with letterSpacing 0.5, cornerRadius 5–8, `$hover` for highlight fill).
- Commit after every task; `.pen` changes are one opaque blob per commit, so keep each commit one storyboard.

### Node ID snapshot (verify before use)

| ID | What it is |
|---|---|
| `HbhAn` / `Jz0XP` / `Infpe` | Components: Tree Item / Card Row / Card Tile |
| `TtExg` | `Desktop — Add flow (type-ahead)` screen, 1440×900 at x:80 y:1249 — becomes M2 |
| `KMq8b` | `Quick Add Open` frame inside TtExg (Active Field + QA Panel) — the close-up source |
| `bo0fW` → `BLiRd` | Active Field → typed text (`lightn`) |
| `MAr0w` | QA Panel (sections, rows, footer) |
| `c7EeaN` / `fO4tb` | Section "IN THIS COLLECTION" / row Lightning Bolt (count 3) |
| `HXmnO` / `Kkcwd` / `mAky6` / `c7Qw4` | Section "ADD FROM CATALOG" / rows Strike (highlighted, has chip) / Helix / Greaves |
| `kfc9O` → `TaaSS` | `⏎ Have` chip on Strike row → its label text |
| `dFwEk` → `l3VmZX`,`e6GrNc`,`i7x8w`,`TzRuK` | Footer hints: `↑↓ navigate`, `⏎ add 1 here`, `⇧⏎ set count`, `⌥⏎ want instead` |
| `c8IF8` | `Rows` list inside TtExg main panel (Card Row instances) |
| `r7Ov4h` → `fXmRu` | Undo toast → toast text |
| `NobY6` / `nXgzb` / `G0lls` | Desktop screens: Collection view / Catalog search / Sign in |
| `N3ptHh` | `Mobile — My cards root` (other mobile screens: find by name, they're in the unread "+4") |

### Card Row component descendant keys (used when adding rows to `c8IF8`)

From existing instances: `pXv7U` = name text, `c7eYH` = HERE count, `X4oXVa` = WANTED count, `H1Luw` = OWNED count, `AXo1h` = child-collection affordance (disable with `enabled:false`).

### Shared cell recipe (used by Tasks 2–6)

Every state is a **cell**: a vertical frame `{gap: 8}` whose first child is a caption and second child is the state frame.

```js
cell=Insert(container,{type:"frame",name:"Cell S2",layout:"vertical",gap:8})
Insert(cell,{type:"text",name:"Caption S2",fontFamily:"$font",fontSize:12,fontWeight:"600",fill:"$text-2",content:"S2 · types \"ligh\""})
```

Close-up state frames are `{type:"frame",name:"State ...",layout:"vertical",width:720,padding:16,gap:6,fill:"$bg",stroke:"$border",strokeWidth:1,cornerRadius:8}` with the `KMq8b` copy inserted inside (`KMq8b`'s root is `width:"fill_container"`, so it adapts).

---

### Task 1: Preflight + storyboard region scaffold

**Files:**
- Modify: `design/wireframes.pen` (via pencil MCP only)

**Interfaces:**
- Produces: four root container frames named exactly `Proto — Add flow · Desktop`, `Proto — Add flow · Deck context`, `Proto — Add flow · Catalog`, `Proto — Add flow · Mobile` — every later task inserts cells into these by name/ID.

- [ ] **Step 1: Check for pre-existing uncommitted `.pen` changes**

Run: `git status --porcelain design/wireframes.pen`
If it prints `M design/wireframes.pen`, commit the pre-existing state first so prototype commits are clean:

```bash
git add design/wireframes.pen && git commit -m "chore(design): checkpoint wireframes.pen before add-flow prototype"
```

- [ ] **Step 2: Verify document state and IDs**

Call `mcp__pencil__batch_get` with `{filePath: "design/wireframes.pen", nodeIds: ["TtExg","KMq8b","NobY6","nXgzb","N3ptHh"], readDepth: 1}` and `patterns: [{name: "^Mobile"}], searchDepth: 1` in the same call. Confirm the ID table above; note the IDs/names of all mobile screens for Task 6. If anything moved, update your working notes — not this plan file.

- [ ] **Step 3: Create the four containers**

One `batch_design` call. Chain `FindEmptySpace` downward so rows stack vertically with 120px padding:

```js
p1=FindEmptySpace({width:7200,height:1100,direction:"bottom",padding:120,nodeId:"TtExg"})
row1=Insert(document,{type:"frame",name:"Proto — Add flow · Desktop",x:p1.x,y:p1.y,layout:"horizontal",gap:48,padding:24,alignItems:"start",placeholder:true})
Insert(row1,{type:"note",name:"Board Title Desktop",content:"Add flow · Desktop type-ahead (binder, Have-led) — walk M1→S1→S2→S3→M2; S4/S5 are ⇧⏎/⌥⏎ variants",width:280,fontSize:13})
```

Repeat for the other three rows, each anchored on the previous row's ID (`nodeId: row1` etc.), names `Proto — Add flow · Deck context` (est. 1800×700), `Proto — Add flow · Catalog` (est. 3200×1100), `Proto — Add flow · Mobile` (est. 1600×1000), each with its own title note (`Deck context (Grixis, Want-led) — only the default flips`, `Catalog surface — destination picker + quick actions + logged-out`, `Mobile intake (Inbox) — tap accounting`).

- [ ] **Step 4: Verify**

`snapshot_layout({filePath, maxDepth: 0})` — confirm the four rows exist below existing screens with no overlaps.

- [ ] **Step 5: Commit**

```bash
git add design/wireframes.pen && git commit -m "design(wireframes): scaffold add-flow prototype storyboard region"
```

---

### Task 2: Desktop close-ups S1–S5

**Files:**
- Modify: `design/wireframes.pen` — insert into `Proto — Add flow · Desktop`

**Interfaces:**
- Consumes: container `row1` (Task 1); `KMq8b` and its descendant IDs (snapshot table).
- Produces: seven cells in walk order — `Cell M1` and `Cell M2` created here as caption-only placeholders (state frames filled by Task 3); S-cells complete.

**Order matters:** these copies MUST be taken before Task 3 edits `TtExg`/`KMq8b` into M2.

- [ ] **Step 1: Create all seven cells in walk order**

One `batch_design`: cells named `Cell M1`, `Cell S1`, `Cell S2`, `Cell S3`, `Cell M2`, `Cell S4`, `Cell S5` inserted into the Desktop container in that order, each per the shared cell recipe with captions:
`M1 · at rest — field always present (/ focuses)` · `S1 · / — focused, empty` · `S2 · types "ligh"` · `S3 · ↓ — disambiguate (1 keystroke/step)` · `M2 · ⏎ — landed, field clear, loop restarts` · `S4 · ⇧⏎ — set count` · `S5 · ⌥⏎ — want instead`.

- [ ] **Step 2: S1 (empty focused) — copy with sections disabled**

```js
s1=Insert(cellS1,{type:"frame",name:"State S1",layout:"vertical",width:720,padding:16,gap:6,fill:"$bg",stroke:"$border",strokeWidth:1,cornerRadius:8})
Copy("KMq8b",s1,{name:"QA S1",descendants:{"BLiRd":{content:"Add or find cards…",fill:"$text-3"},"c7EeaN":{enabled:false},"fO4tb":{enabled:false},"HXmnO":{enabled:false},"Kkcwd":{enabled:false},"mAky6":{enabled:false},"c7Qw4":{enabled:false}}})
```

- [ ] **Step 3: S2 (typed) and S3 (highlight moved)**

S2 is a near-verbatim copy: `Copy("KMq8b",s2,{name:"QA S2",descendants:{"BLiRd":{content:"ligh"}}})`.
S3 moves the highlight from Strike to Helix — un-fill Strike, drop its chip, replace the Helix row with a highlighted version carrying the chip:

```js
Copy("KMq8b",s3,{name:"QA S3",descendants:{"BLiRd":{content:"ligh"},"Kkcwd":{fill:"#FFFFFF"},"kfc9O":{enabled:false},"mAky6":{type:"frame",name:"QA Row Helix Hi",alignItems:"center",gap:8,padding:[7,8],cornerRadius:5,fill:"$hover",width:"fill_container",children:[{type:"text",name:"Helix Name",content:"Lightning Helix",fontFamily:"$font",fontSize:13,fontWeight:"500",fill:"$text",textGrowth:"fixed-width",width:"fill_container"},{type:"text",name:"Helix Meta",content:"RVR · RW",fontFamily:"$font",fontSize:11,fill:"$text-3"},{type:"frame",name:"Helix Enter Chip",cornerRadius:4,fill:"$text",padding:[3,8],children:[{type:"text",name:"Helix Enter Label",content:"⏎ Have",fontFamily:"$font",fontSize:11,fontWeight:"500",fill:"#FFFFFF"}]}]}}})
```

- [ ] **Step 4: S4 (count entry) and S5 (want committed)**

S4 = S2 copy but the chip becomes an inline count field:

```js
Copy("KMq8b",s4,{name:"QA S4",descendants:{"BLiRd":{content:"ligh"},"kfc9O":{type:"frame",name:"Count Entry",alignItems:"center",gap:4,cornerRadius:4,fill:"#FFFFFF",stroke:"$accent",strokeWidth:1.5,padding:[3,8],children:[{type:"text",name:"Count X",content:"×",fontFamily:"$font",fontSize:11,fill:"$text-3"},{type:"text",name:"Count Value",content:"4",fontFamily:"$font",fontSize:12,fontWeight:"600",fill:"$text"},{type:"text",name:"Count Commit",content:"⏎",fontFamily:"$font",fontSize:11,fill:"$text-3"}]}}})
```

S5 = S2 copy with the chip flipped to a committed-want treatment:

```js
Copy("KMq8b",s5,{name:"QA S5",descendants:{"BLiRd":{content:"ligh"},"kfc9O":{type:"frame",name:"Wanted Chip",cornerRadius:4,fill:"#FFFFFF",stroke:"$accent",strokeWidth:1,padding:[3,8],children:[{type:"text",name:"Wanted Label",content:"✓ wanted 1",fontFamily:"$font",fontSize:11,fontWeight:"500",fill:"$accent"}]}}})
```

- [ ] **Step 5: Verify**

`snapshot_layout({parentId: row1, problemsOnly: true})` → expect no problems. Do NOT screenshot yet (row incomplete until Task 3).

- [ ] **Step 6: Commit**

```bash
git add design/wireframes.pen && git commit -m "design(wireframes): add-flow proto — desktop close-ups S1–S5"
```

---

### Task 3: Desktop milestones M1 + M2

**Files:**
- Modify: `design/wireframes.pen` — fill `Cell M1`/`Cell M2`; `TtExg` is renamed and moved

**Interfaces:**
- Consumes: `Cell M1`/`Cell M2` (Task 2), `TtExg` + descendants (snapshot table).
- Produces: `TtExg` now lives inside the Desktop container as M2 — anything else referencing it as a standalone screen is gone (that is intended, per design: "adjusted into M2 rather than duplicated").

- [ ] **Step 1: M1 — copy TtExg into an at-rest screen**

Copy BEFORE editing TtExg. Replace the open quick-add with an idle field, disable the toast:

```js
m1=Copy("TtExg",cellM1,{name:"Proto M1 — At rest",placeholder:true,descendants:{"r7Ov4h":{enabled:false},"KMq8b":{type:"frame",name:"Quick Add Idle",layout:"vertical",gap:6,width:"fill_container",children:[{type:"frame",name:"Idle Field",alignItems:"center",gap:8,height:36,padding:[0,10],cornerRadius:6,fill:"#FFFFFF",stroke:"$border",strokeWidth:1,width:"fill_container",children:[{type:"icon",name:"Idle Search Icon",library:"lucide",icon:"search",width:16,height:16,fill:"$text-3"},{type:"text",name:"Idle Placeholder",content:"Add or find cards…",fontFamily:"$font",fontSize:14,fill:"$text-3",textGrowth:"fixed-width",width:"fill_container"},{type:"frame",name:"Slash Hint",cornerRadius:4,fill:"$hover",padding:[2,7],children:[{type:"text",name:"Slash Label",content:"/",fontFamily:"$font",fontSize:11,fill:"$text-2"}]}]}]}}})
Update(m1,{placeholder:false})
```

- [ ] **Step 2: M2 — adjust TtExg in place, then move it into the cell**

One `batch_design`: clear the typed text to placeholder, empty the suggestion sections (keep footer), add the landed row to the collection list, keep the toast, rename, move:

```js
Update("BLiRd",{content:"Add or find cards…",fill:"$text-3"})
Update("c7EeaN",{enabled:false});Update("fO4tb",{enabled:false});Update("HXmnO",{enabled:false});Update("Kkcwd",{enabled:false});Update("mAky6",{enabled:false});Update("c7Qw4",{enabled:false})
```

Add the landed row as a fresh Card Row instance, placed right after the Foils row (index 2 keeps the column header + Foils above):

```js
strike=Insert("c8IF8",{type:"ref",ref:"Jz0XP",name:"Lightning Strike (landed)",width:"fill_container",fill:"$selected",descendants:{"AXo1h":{enabled:false},"pXv7U":{content:"Lightning Strike",fontWeight:"400"},"c7eYH":{content:"1"},"X4oXVa":{content:""},"H1Luw":{content:""}}})
Move(strike,"c8IF8",2)
Update("TtExg",{name:"Proto M2 — Landed (⏎)"})
Move("TtExg",cellM2)
```

(The toast `r7Ov4h` already reads `Added 1 Lightning Strike to Trade Binder` — leave it.)

- [ ] **Step 3: Verify the whole Desktop row**

`snapshot_layout({parentId: row1, problemsOnly: true})` → no problems. Then ONE `get_screenshot({nodeId: row1})` — check: M1 idle field reads as always-present; S2/S3 highlight movement is legible; M2 shows cleared field + new `$selected` row + toast; captions match states.

- [ ] **Step 4: Fix anything the screenshot shows, by direct Update (never delete/recreate), then commit**

```bash
git add design/wireframes.pen && git commit -m "design(wireframes): add-flow proto — desktop milestones M1/M2, TtExg becomes M2"
```

---

### Task 4: Deck-context storyboard D1–D2

**Files:**
- Modify: `design/wireframes.pen` — insert into `Proto — Add flow · Deck context`

**Interfaces:**
- Consumes: deck container (Task 1); `KMq8b` descendants — NOTE: after Task 3, `KMq8b`'s live state is M2 (sections disabled). Therefore D1/D2 copy from the Task 2 artifacts instead: use the `QA S2` copy inside `Cell S2` as the source. Get its new ID via `batch_get` `patterns:[{name:"^QA S2$"}]`.
- Produces: two complete cells, `Cell D1`, `Cell D2`.

- [ ] **Step 1: D1 — Want-led copy of QA S2**

Cells per shared recipe, captions `D1 · deck context — default flips to Want` and `D2 · ⏎ — wanted +1, needs chip ticks`. Copying the S2 copy: its descendant IDs are NEW (assigned at Task 2 copy time), so first `batch_get` the `QA S2` subtree (`readDepth: 4`) and note the IDs of: typed text (name `QA Typed`), chip label (name `QA Enter Label`), footer hints (names `QA Hint Enter`, `QA Hint Want`). Then:

```js
d1=Insert(cellD1,{type:"frame",name:"State D1",layout:"vertical",width:720,padding:16,gap:6,fill:"$bg",stroke:"$border",strokeWidth:1,cornerRadius:8})
Copy(qaS2id,d1,{name:"QA D1",descendants:{[chipLabelId]:{content:"⏎ Want"},[hintEnterId]:{content:"⏎ want 1"},[hintWantId]:{content:"⌥⏎ have instead"}}})
```

- [ ] **Step 2: D2 — landed panel with deck header strip**

Built fresh (no good copy source) inside `State D2` (same 720 frame recipe):

```js
hdr=Insert(d2,{type:"frame",name:"Deck Header Strip",alignItems:"center",justifyContent:"space_between",width:"fill_container",padding:[0,2]})
Insert(hdr,{type:"text",name:"Deck Name",content:"Grixis Control",fontFamily:"$font",fontSize:14,fontWeight:"600",fill:"$text"})
chip=Insert(hdr,{type:"frame",name:"Needs Chip",cornerRadius:12,fill:"$hover",padding:[3,10],gap:4,alignItems:"center"})
Insert(chip,{type:"text",name:"Needs Text",content:"7 missing — 4 owned elsewhere · 3 to buy",fontFamily:"$font",fontSize:11,fontWeight:"500",fill:"$text-2"})
```

then an Active-Field clone (same structure as M1's Idle Field but stroke `$accent` strokeWidth 1.5, placeholder text), then one landed row: horizontal frame `{padding:[7,8],cornerRadius:5,width:"fill_container",alignItems:"center",gap:8}` with texts `Lightning Strike` (13px `$text`) and right-aligned `wanted 1` (11px `$accent`, fontWeight 500).

- [ ] **Step 3: Verify + commit**

`snapshot_layout({parentId: deckContainer, problemsOnly: true})` → none; ONE screenshot of the deck container — check the ONLY differences from the binder walk are chip/hints/header (that's the storyboard's argument).

```bash
git add design/wireframes.pen && git commit -m "design(wireframes): add-flow proto — deck-context flip D1/D2"
```

---

### Task 5: Catalog storyboard C1–C3

**Files:**
- Modify: `design/wireframes.pen` — insert into `Proto — Add flow · Catalog`

**Interfaces:**
- Consumes: catalog container (Task 1); `nXgzb` (Desktop — Catalog search) and `Infpe` (Card Tile) — both structurally unread; this task starts by reading them.
- Produces: cells `Cell C1` (full screen), `Cell C2`, `Cell C3` (close-ups).

- [ ] **Step 1: Read the sources**

`batch_get {nodeIds: ["nXgzb","Infpe"], readDepth: 4}`. In `nXgzb`, locate by name the results toolbar / `Adding to:` destination picker node and note its ID and position (`snapshot_layout({parentId: "nXgzb", maxDepth: 3})` for coordinates). In `Infpe`, note descendant IDs for name text, meta/set text, and any quick-action nodes.

- [ ] **Step 2: C1 — full screen with picker open**

Cells per shared recipe, captions: `C1 · destination picker open — set once per session`, `C2 · + Have — lands in picked target`, `C3 · logged out — actions become sign-in`.

```js
c1s=Copy("nXgzb",cellC1,{name:"Proto C1 — Picker open",placeholder:true})
```

Then insert the dropdown as an absolutely positioned panel inside the copied screen, x/y placed directly under the picker's coordinates from Step 1:

```js
dd=Insert(c1s,{type:"frame",name:"Dest Dropdown",layoutPosition:"absolute",x:PICKER_X,y:PICKER_Y_BOTTOM,layout:"vertical",width:280,padding:6,gap:2,fill:"#FFFFFF",stroke:"$border",strokeWidth:1,cornerRadius:8,effect:{type:"shadow",shadowType:"outer",offset:{x:0,y:4},blur:16,color:"#00000014"}})
```

Children, in order (all `width:"fill_container"`, structure mirroring the QA panel rows: 13px name + 11px `$text-3` meta, section labels 10px letterSpacing 0.5): search field (`Find collection…` placeholder, 32px), `SUGGESTED` label, row `Grixis Control — wants 2` (fill `$hover`, it's the pre-highlight), `RECENT` label, row `Trade Binder`, divider (1px `$border` rect in a `[6,4]` padded wrap), rows `All cards · 812`, `Inbox · 7`, `Binders ▸`, `Decks ▸`. Unset `placeholder` when done.

- [ ] **Step 3: C2 and C3 — tile close-ups**

`State C2` frame (recipe, but `width: 320` — tile-sized): a `Card Tile` instance (`ref: "Infpe"`) named `Lightning Strike`, with a count badge overlaid (absolute frame, top-right: `cornerRadius:10,fill:"$text",padding:[2,8]`, text `×1` 11px #FFFFFF) — override tile name/meta descendants per Step 1's IDs. Below the tile, a toast frame (copy the structure of `r7Ov4h`: dark fill `$text`, `cornerRadius:8,padding:[10,16],gap:14`) with texts `Added 1 → Inbox` (13px #FFFFFF) and `Undo` (13px 600 #93C5FD).
`State C3` (same 320 recipe): another tile instance; disable its quick-action descendants (`enabled:false` via override) and add below the tile a sign-in prompt row: frame `{alignItems:"center",gap:6,padding:[7,10],cornerRadius:5,stroke:"$border",strokeWidth:1}` with lucide `log-in` icon 14px `$text-2` and text `Sign in to add` (12px, 500, `$text-2`). If `Infpe` turns out to have no quick-action nodes (spec findings imply actions may live on hover), skip the disable and let the prompt row carry the state alone.

- [ ] **Step 4: Verify + commit**

Problems-scan the catalog container; ONE screenshot; check dropdown doesn't clip outside the copied screen (screen frames have `clip:true` — if the dropdown would clip, it's positioned wrong, fix the y).

```bash
git add design/wireframes.pen && git commit -m "design(wireframes): add-flow proto — catalog C1–C3 (picker, quick add, logged-out)"
```

---

### Task 6: Mobile storyboard Mb1–Mb3

**Files:**
- Modify: `design/wireframes.pen` — insert into `Proto — Add flow · Mobile`

**Interfaces:**
- Consumes: mobile container (Task 1); the mobile collection screen identified in Task 1 Step 2 (expected name like `Mobile — Collection` / drill-down; fall back to `N3ptHh` My cards root if none exists).
- Produces: cells `Cell Mb1`–`Cell Mb3`, three full 390×844 screens.

- [ ] **Step 1: Read the mobile source screen**

`batch_get` the chosen mobile screen `readDepth: 4`; note its header structure, list container ID, and whether it already has a search/type-ahead field.

- [ ] **Step 2: Mb1 — Inbox at rest**

Captions: `Mb1 · Inbox — field in thumb reach`, `Mb2 · focused — suggestions above keyboard, tap +Have`, `Mb3 · added — toast, field clear`.
`Copy` the source screen into `Cell Mb1` (name `Proto Mb1 — Inbox`, `placeholder:true`), overriding via `descendants`: title → `Inbox`, and if no type-ahead exists, insert one directly under the header — same Idle Field structure as Task 3 M1 (36px, `$border` stroke, search icon, `Add or find cards…`) full-width with `[0,12]` side padding. Keep list rows; rename two to `Lightning Bolt`/`Opt` if the copied content is collection-specific. Unset placeholder.

- [ ] **Step 3: Mb2 — keyboard up, suggestions**

Copy Mb1's screen into `Cell Mb2` (get its new ID from the Step 2 return; overrides via the Copy's `descendants` where possible). Then in the copy: field stroke → `$accent` strokeWidth 1.5 with typed text `ligh` (fill `$text`); insert at screen bottom an absolute keyboard block `{layoutPosition:"absolute",x:0,y:564,width:390,height:280,fill:"$hover"}` containing a centered label `keyboard` (11px `$text-3`); insert an absolute suggestions panel directly above it `{x:8,y:414,width:374}` (white, `$border` stroke, cornerRadius 8, padding 6, vertical gap 2) with three rows — each `{alignItems:"center",gap:8,padding:[10,8],cornerRadius:5,width:"fill_container"}`, name 13px + meta 11px, and a `+ Have` chip (min 28px tall frame, `$text` fill, white 11px label — the 44px tap target comes from row padding + chip): `Lightning Strike · DMU · 1R` (row fill `$hover`), `Lightning Helix · RVR`, `Lightning Greaves · CMM`.

- [ ] **Step 4: Mb3 — landed**

Copy Mb2's screen into `Cell Mb3`; overrides/edits: typed text back to placeholder (`Add or find cards…`, `$text-3`), field stroke stays `$accent` (still focused), delete the suggestions panel node, add `Lightning Strike` row into the list (HERE-style count `1`), insert toast above keyboard `{layoutPosition:"absolute",x:20,y:504,width:350}` — dark toast structure from Task 5 with text `Added 1 Lightning Strike to Inbox` + `Undo`.

- [ ] **Step 5: Verify + commit**

Problems-scan mobile container; ONE screenshot; check the keyboard block doesn't cover the toast and tap chips read ≥ comfortable size.

```bash
git add design/wireframes.pen && git commit -m "design(wireframes): add-flow proto — mobile intake Mb1–Mb3"
```

---

### Task 7: Accounting notes + final review

**Files:**
- Modify: `design/wireframes.pen` — one note per storyboard + one summary note

**Interfaces:**
- Consumes: all four containers.
- Produces: the validation deliverable (the numbers the spec Findings will cite).

- [ ] **Step 1: Insert accounting notes (exact copy below)**

One `batch_design`; each note appended as the LAST child of its container (`width: 300, fontSize: 12`):

- Desktop: `Cost/card steady state: ~4–6 chars + ⏎ = 5–7 keystrokes, zero pointer. 50 cards ≈ 250–350 keystrokes. Detours: ↓ ×n disambiguation; ⇧⏎ + digits + ⏎ set count.`
- Deck: `Identical rhythm to binder walk; only the default flips (⏎ = want, ⌥⏎ = have). No added cost.`
- Catalog: `Destination set once per session (picker). Then 1 click (+Have/+Want) per result — search cost dominates; not the 50-card path.`
- Mobile: `1 tap focus (once) + ~5 chars + 1 tap on match per card. Keyboard stays up between adds.`

Then a summary note at the END of the Desktop container (`width: 340`): `VALIDATION — 50-card projection: 250–350 keystrokes desktop, no pointer; mobile ≈ 50 taps + ~250 chars. Risks to watch in use: (1) how often 4–6 chars still needs ↓ disambiguation; (2) whether ⇧⏎ set-count is cheap enough for playset entry (4× same card).`

- [ ] **Step 2: Final pass**

`snapshot_layout({problemsOnly: true})` on the whole document → fix any problems by direct Update. Confirm every prototype frame has `placeholder` unset. ONE `get_screenshot` per any container you changed during fixes.

- [ ] **Step 3: Commit**

```bash
git add design/wireframes.pen && git commit -m "design(wireframes): add-flow proto — input-cost accounting notes"
```

---

### Task 8: Findings, TODO flip, follow-ups

**Files:**
- Modify: `specs/ui-design.md` (Findings section)
- Modify: `specs/TODO.md` (Phase 1b task 3 `[~]` → `[x]`)

**Interfaces:**
- Consumes: the accounting numbers (Task 7) — if execution changed them, the Findings text changes with them.

- [ ] **Step 1: Add the Findings entry**

Append to the Findings list in `specs/ui-design.md` (Edit tool; markdown, first-person-project voice matching existing entries):

```markdown
- 2026-07-10 — **Add-to-collection flow prototyped** (Phase 1b task 3); deliverable: storyboard region in design/wireframes.pen (`Proto — Add flow · Desktop / Deck context / Catalog / Mobile` — states M1/S1–S5/M2, D1–D2, C1–C3, Mb1–Mb3), built per [design/add-flow-prototype.md](../design/add-flow-prototype.md). Pencil has no interactive links, so "click-through" is a captioned state walk with explicit input-cost accounting: desktop steady state ≈ 5–7 keystrokes/card, zero pointer (50 cards ≈ 250–350 keystrokes); deck context changes nothing but the default (⏎ = want); catalog adds are 1 click/result after a once-per-session destination pick; mobile ≈ 1 tap + ~5 chars/card. Risks flagged for real-usage validation: disambiguation frequency at 4–6 typed characters, and whether ⇧⏎ set-count stays cheap for playset entry.
```

- [ ] **Step 2: Flip the TODO checkbox**

In `specs/TODO.md`, change `- [~] Prototype the add-to-collection flow …` to `- [x] Prototype the add-to-collection flow (specs: [ui-design](ui-design.md)) — storyboards in design/wireframes.pen + input-cost accounting; see spec Findings`.

- [ ] **Step 3: Record follow-ups (only if discovered during execution)**

Any component gap noticed (e.g., destination-picker dropdown, count stepper, toast) belongs to the NEXT task ("Component gap analysis") — do not add TODO entries for those. Add new `[ ]` TODO tasks only for genuinely new work outside that task's scope.

- [ ] **Step 4: Final commit (findings + flip together — this is the DoD commit)**

```bash
git add specs/ui-design.md specs/TODO.md && git commit -m "design: add-to-collection flow prototype complete — storyboards + input-cost accounting"
```

---

## Self-Review Notes

- Spec coverage: design doc storyboard tables map 1:1 — Desktop M1/S1–S5/M2 (Tasks 2–3), Deck D1–D2 (Task 4), Catalog C1–C3 (Task 5), Mobile Mb1–Mb3 (Task 6), accounting notes (Task 7), DoD wiring (Task 8). Canvas-organization requirements (region naming, captions-as-ledger, component reuse, TtExg→M2) are embedded in Tasks 1–3.
- Known execution-time unknowns are made explicit read-first steps (Task 5 Step 1, Task 6 Step 1) because `nXgzb`, `Infpe`, and the mobile screens were not structurally read at planning time — do not skip those reads.
- Copy-descendant rule: every state override rides the `Copy` call itself; the one place that needs a post-copy read (Task 4, copying a Task-2 artifact) says so explicitly.
