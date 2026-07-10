# Command Palette Wireframes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the `⌘K — Command palette` region (trigger note + frames P1 at-rest and P2 typed) in design/wireframes.pen per [design/command-palette.md](../../../design/command-palette.md), then close the task.

**Architecture:** One new top-level container frame in the overlay band of the canvas (right of the hover-preview overlay), holding a documentation note and two cells (caption + palette frame each), built from the canvas's established idioms: quick-add panel (focused field, group labels, rows, kbd footer) and destination dropdown (name + meta rows).

**Tech Stack:** Pencil MCP batch_design; git.

## Global Constraints

- `.pen` files are encrypted — Pencil MCP tools only; the editor flushes to disk only on the user's manual Cmd+S, so the final commit waits for their save.
- Reuse idiom values verbatim from existing nodes: focused field = `stroke $accent / strokeWidth 1.5` (QA `Active Field`); group label = fontSize 10, `$text-3`, letterSpacing 0.5; row = padding [7,8], cornerRadius 5, selected fill `$hover`; name text 13 `$text` fill_container + meta 11 `$text-3`; kbd footer = padding [8,8,4,8], top border `$border`, texts 11 `$text-3`.
- Placement: region container at x:2600, y:1249 (Sign in screen above ends at y≈1129; mobile band starts at x:4980 — no overlap). Verify with a problems-only snapshot_layout after build; the only pre-existing expected problem is the intentional `ERjim`/`kjlXK` filter-sheet clip.
- Queue DoD: findings in specs/ui-design.md; checkbox `[~]`→`[x]` in the same commit as the final work.

---

### Task 1: Build the region

- [ ] **Step 1: Load schema and insert the region in one batch_design**

`get_editor_state(include_schema: true)` first (schema not in context post-compaction). Then one batch_design script inserting into the document:

Container: `{type:"frame", name:"⌘K — Command palette", x:2600, y:1249, padding:24, gap:48}` with three children in order:

1. Note `{type:"note", name:"Note — ⌘K palette", width:300, height:0}` content: "⌘K — Command palette (v1): centered modal over a scrim, logged-in desktop only (hardware-keyboard accelerator; no mobile trigger in v1). ⌘K/Ctrl+K toggles; / stays the quick-add binding. ⏎ opens a place or runs a command. Empty query = RECENT + COMMANDS with the first row pre-selected, so ⌘K ⏎ returns to the last collection. Spec: design/command-palette.md."
2. Cell P1 `{type:"frame", layout:"vertical", gap:8}`: caption text `P1 · at rest — ⌘K` (12, `$text-2`, weight 600) + palette frame P1.
3. Cell P2, same shape: caption `P2 · typed "tra" — fuzzy places` + palette frame P2.

Palette frame shell (both): `{type:"frame", layout:"vertical", width:400, fill:"#FFFFFF", cornerRadius:12, stroke:"$border", strokeWidth:1, padding:6, gap:2, effect:{type:"shadow", shadowType:"outer", blur:24, color:"#0000001F", offset:{x:0,y:6}}}`.

P1 children: field (h:40, horiz, alignItems center, gap 8, padding [0,10], cornerRadius 6, fill #FFFFFF, stroke $accent 1.5): search icon (lucide, 16, `$text-3`) + text "Where to?" (`$text-3`, 14, fill_container) — then label row (padding [6,8,2,8]) "RECENT" — rows: `Trade Binder`/meta `Binders` (fill `$hover`, selected), `Grixis Control`/meta `Decks`, `Shopping list`/no meta — label row (padding [8,8,2,8]) "COMMANDS" — rows: `New binder…`, `New deck…`, `Undo last move` (no metas) — footer (gap 14, padding [8,8,4,8], top stroke `$border`): texts `↑↓ navigate`, `⏎ open`, `esc close`.

P2 children: same field with typed text "tra" (`$text`, 14) — label "COLLECTIONS" — rows: `Trade Binder`/meta `Binders` (fill `$hover`, selected), `Trade duplicates`/meta `Bulk Box` — same footer. (Commands dropped out — no COMMANDS group.)

- [ ] **Step 2: Verify**

Problems-only snapshot_layout (re-run if surprising): expect only the intentional filter-sheet clip. Then one screenshot of the new region to check visual fidelity (alignment, idiom match).

### Task 2: Close out

- [ ] **Step 1: Findings + checkbox**

specs/ui-design.md Findings (top of list): entry recording the palette design (deliverable design/command-palette.md + wireframe region; v1 scope: places + 3 commands, Sign out dropped, no cards, desktop-only). specs/TODO.md: flip `[~]`→`[x]` on the palette task with a one-line result.

- [ ] **Step 2: Wait for user Cmd+S, then single final commit**

`git add design/wireframes.pen specs/ui-design.md specs/TODO.md` — commit `design(wireframes): ⌘K command palette overlays (P1/P2) + note; palette design task done`.
