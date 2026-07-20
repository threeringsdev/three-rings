# Component gap analysis vs. Rust/UI

**Deliverable of:** Phase 1b task "Component gap analysis vs. Rust/UI registry" — see [specs/ui-design.md](../specs/ui-design.md), [specs/ui-components.md](../specs/ui-components.md)
**Approved design:** [docs/superpowers/specs/2026-07-10-component-gap-analysis-design.md](../docs/superpowers/specs/2026-07-10-component-gap-analysis-design.md)
**Registry pin:** [rust-ui/ui](https://github.com/rust-ui/ui) @ `43e1e32` (2026-07-01), components under `app_crates/registry/src/ui/`

## How to read this

Every UI primitive appearing in the Phase 1b wireframes ([wireframes.pen](wireframes.pen): 8 screens, hover-preview overlay, 3 reusable components, 4 add-flow storyboard rows) gets one row and one verdict. **Direct** = registry component adopted as-is (styling edits are always ours — components are vendored and owned per ui-components). **Composite** = assembled from registry parts; the assembly is ours, the parts aren't gaps. **Gap** = nothing in the registry covers it; custom component needed. App-specific composites (three-counts row, card grid) are feature-side work per the ui-components scope line — listed for completeness, not gaps. Registry names below all exist at the pin.

Maturity caveat: the registry is young and uneven — `action_bar` (read for the selection-tray ruling below) ships an SVG-filter experiment marked `// TODO 🐛. Not working yet`. Every Direct verdict therefore means "adopt via the component bench with eyes open," not "guaranteed finished upstream."

## Catalog

Screen legend: DCol/DCat/DSig = desktop collection/catalog/sign-in · OvHP = hover preview · MRoot/MCol/MCat/MFil/MCard = mobile root/collection/catalog/filter sheet/card sheet · Proto = add-flow storyboards.

| Primitive | Screens | Registry match | Verdict |
|---|---|---|---|
| Button (solid/outline, icon) | all | `button` | Direct |
| Segmented toggle (mode switch, grid/list) | DCol DCat | `toggle_group` | Direct |
| Text input (quick search, query bar, fields) | DCol DCat DSig MCol MFil Proto | `input` (+ `input_group` for icon/hint affixes) | Direct |
| Label + field group | DCat DSig MFil | `label` + `field` | Direct |
| Checkbox option row | DCat MFil | `checkbox` + `label` | Direct |
| Collapsible filter section | DCat MFil | `collapsible` | Direct — **adopted 2026-07-20** (tree task): + `aria-expanded`/`aria-controls` (caller-supplied content id) and `inert` when closed — the grid animation keeps collapsed content in the DOM, which left its links tab-reachable |
| Badge / count chip (tree counts, tab badge, ×1 tile badge, needs chip, section counts) | DCol MRoot MCat MFil Proto | `badge` | Direct |
| Keyboard hint (`/`, esc, ↑↓ ⏎ ⇧⏎ ⌥⏎ footer) | DCol Proto | `kbd` | Direct |
| Separator | DCol MRoot Proto | `separator` | Direct |
| Avatar | DCol DCat MRoot | `avatar` | Direct |
| Link (Reset, Clear all, Create account, Full details →) | DCat DSig MFil MCard | `link` | Direct |
| Breadcrumb (desktop path; mobile back row) | DCol MCol | `breadcrumb` (mobile back = `button` variant) | Direct |
| Toast with action (Undo) | Proto (C2 M2 Mb3) | `sonner` | Direct |
| Hover preview shell | OvHP | `hover_card` | Direct (content is feature-side) |
| Bottom sheet (grabber, scrim, sticky action) | MFil MCard | `sheet` | Direct (grabber styling ours) |
| Mobile tab bar with badge | MRoot MCol MCat | `bottom_nav` + `badge` | Direct — verify maturity at adoption |
| Drill-down list row (icon · label · count · ›) | MRoot | `item` | Direct — **adopted 2026-07-20** (tree task): `variants!` hand-expanded (V1 convention); `support_href` became a real `href` prop rendering an `<a>`, with upstream's `[a]:`-arbitrary-variant hover classes moved onto that arm as plain utilities (the `[a]:` form resolves to no usable CSS here) |
| Modal dialog (move confirm, teardown preview — spec'd flows, not drawn) | per ui-design flows | `dialog` / `alert_dialog` | Direct |
| Placeholder bars (oracle text) | OvHP MCard | `skeleton` | Direct (at build time) |
| Theme toggle (open theming question) | — | `theme_toggle` | Direct (when theming lands) |
| Quick-add panel (grouped suggestions, row action chips, inline count entry, kbd footer; mobile: docked above keyboard) | Proto (all rows) | `command` + `popover` + `input` + `kbd` | Composite |
| Destination picker (chip trigger; search + SUGGESTED/RECENT + tree dropdown) | DCat MCat Proto (C1) | `popover` + `command` | Composite |
| Global ⌘K command palette (v1 per maintainer decision) | — | `command` + `dialog` | Composite |
| Auth card | DSig | `card` + `field` + `input` + `button` | Composite |
| Collection tree (nested, collapsible, drag-reparent/reorder, pinned rows, selection, badges) | DCol | — | **Gap** |
| In-place count stepper (hover − n + / click-to-type / focused-row ±) | DCol | — | **Gap** |
| Selection tray (docked: thumbnail stack, count, Move to…, clear) | MRoot (docks on desktop too per IA) | — | **Gap** |

Feature-side composites (not registry gaps, live with their features per ui-components scope): card row + three-counts columns (on `table` parts, already vendored), card tile + results grid (`card`/`aspect_ratio`/`image`), hover-preview and card-sheet content, drill-down screens.

**V1 adoption notes (2026-07-19, vendored from the 43e1e32 pin):** button,
badge, input, input_group, kbd, separator, checkbox, label, toggle_group,
breadcrumb, skeleton, and card are in `app/src/components/ui/`. Batch-wide
deviations (full list per file header): `variants!` hand-expanded to enums +
match (no `leptos_ui`/`paste`); button `Warning`/`Success`/`Bordered` and
badge `Success`/`Warning`/`Info` variants dropped — they reference tokens
`style/input.css` doesn't define, so Tailwind would silently emit no CSS
(re-add variant + token together); icons inlined (Lucide paths) instead of
the registry icons crate; label's runtime-named peer classes replaced with
the static `peer-disabled:` pair (Tailwind can't generate CSS for
runtime-built class names — upstream bug for any Tailwind build);
`InputGroupTextarea` dropped with textarea unvendored; input's `strum` enum
hand-written.

**V2 adoption notes (2026-07-19, overlay foundations):** `scroll_lock` (the
pure-Rust registry hook, not the JS asset), `dialog`, `popover`, `sheet` are
in `app/src/components/ui/`, plus a new `overlay_stack` (ours). "Vendor
markup + CSS, rewire behavior" per the review above: caller-supplied
deterministic IDs, one `RwSignal<bool>` per overlay driving trigger/close/
backdrop/ESC, no inline scripts. Beyond the six cross-cutting findings, the
Codex review of the rewrite surfaced real multi-overlay bugs now fixed:
reference-counted + generation-guarded scroll lock (stacked overlays / close-
reopen races), a topmost-only ESC stack (one press closes one overlay),
`inert` + `aria-label` on closed panels. Popover keeps the native API + CSS
anchor positioning (confirmed on Android Chrome 145) with a
`css::supports`-gated JS positioning fallback for engines without anchors.

**V3 adoption notes (2026-07-19, interactive core):** `command`,
`hover_card`, `sonner`. `command` is the headline rewrite — the vanilla-JS
keyboard/filter script is replaced by a reactive Leptos item registry
(mount/cleanup registration; ↑↓/⏎ over the visible subset), the shared core
of quick-add / destination picker / ⌘K. `hover_card` keeps the
anchor-positioned native popover with Leptos hover-intent timers. `sonner` is
a **native Leptos toaster written fresh** (not the vendored registry markup)
per the maintainer's engine decision — programmatic `ToastHandle::show` with
an optional action button for undo, so toast state is first-class Leptos
rather than a separate JS engine.

## Gaps

**Collection tree.** The registry has no tree view. Ours needs: arbitrary nesting with per-node collapse, drag to reparent AND reorder, pinned system rows (All cards, Inbox, shopping list), selection state, per-node rolled-up count badges, and context-menu tree management. Nearest parts to build on: `collapsible` (per-node expand), `button`/`item` (row chrome), `badge` (counts), `context_menu` (management), and the registry's `drag_and_drop` primitive — worth evaluating as the drag layer before reaching for a custom one, same maturity caveat as everything else. The tree is the app's central navigation surface; expect it to be the largest custom component.

**In-place count stepper.** No number/stepper input exists in the registry (`input_otp`/`input_phone` are the only specialized inputs). Ours is small but behavior-dense: hidden until row hover/focus, − n + buttons, click-to-type, keyboard ± on a focused row, and commit-on-blur semantics per the collection-view spec. Compose from `button` + `input`; the interaction logic is the work.

**Selection tray.** `action_bar` was read at the pin as the candidate and ruled out: it is a radio-input segmented toolbar built around a liquid-glass SVG-filter experiment — external `/app_components/action_bar.js` script tag, CSS `anchor-name` positioning, inline `<style>` block, `leptos_ui::void` (nightly hazard), and a `// TODO 🐛. Not working yet` marker on its button component. Wrong shape (exclusive selection, not a batch tray) and demo-grade besides. The tray is a custom docked container — thumbnail stack, live count, primary action, clear — composed from `button`/`badge`; the cross-view selection state behind it is app logic anyway.

## SSR/hydration code review

Sources read in full at the pin (`app_crates/registry/src/ui/*.rs` @ `43e1e32`), checked for: client-only APIs at render time, portals, effect-driven first paint, `leptos_ui` (nightly) dependency, non-deterministic IDs, CSR-assuming event wiring. The spike already verified `table` at runtime.

**Cross-cutting findings (apply to all six):**

1. **SSR rendering is safe everywhere.** No component touches `window`/`document` from Rust at render time; all render deterministic closed/static markup on the server. The risks below are hydration- and client-behavior-grade, not SSR failures.
2. **`use_random_id` is a hydration bug waiting** (`hooks/use_random.rs`): IDs come from hashing a process-global `AtomicUsize`. The long-lived server's counter advances across requests while the client WASM restarts at 1 every load — so from the server's second render onward, SSR'd `id=`/`popovertarget=`/script-embedded IDs disagree with what hydration recomputes. Affects `dialog`, `popover`, `hover_card`, `sheet`. Deviation: deterministic caller-supplied IDs (as `command`'s dialog already does via its `id` prop).
3. **All six import `leptos_ui`** (`clx!`/`void!`) — the nightly-feature hazard the spike hit; every adoption gets the vendored [clx.rs](../app/src/components/ui/clx.rs) treatment like `table` did.
4. **Behavior lives in inline vanilla-`<script>` tags**, not Leptos — open/close, keyboard nav, hover timers manipulate the DOM directly, several referencing a `window.ScrollLock` global defined by a *separate* registry item (`use_scroll_lock`), which must be vendored too or the open handlers throw. Fine for static sites; for us, anything the app must drive programmatically (open a move dialog from the `m` key, fire an undo toast after an action) needs rewiring as Leptos-controlled state. The `data-state`-attribute + CSS design underneath is sound and worth keeping.

| Component | Verdict | Evidence |
|---|---|---|
| `dialog` | Adopt with deviations | SSR-safe closed markup (`data-state="closed"`). Deviations: counter ID (`use_random_id_for("dialog")`, dialog.rs:38); `window.ScrollLock.lock()` in the open handler (inline script) — undefined unless `use_scroll_lock` is vendored; per-instance `document` ESC listeners are never removed; open/close is JS-only — programmatic open needs Leptos rewiring. |
| `popover` | Adopt with deviations | Native Popover API (`popover="auto"`, `popovertarget`) + CSS anchor positioning (`position-anchor`, `anchor()`, `position-try-fallbacks`, popover.rs:45–95). SSR-safe. Deviations: counter ID (popover.rs:41); **anchor positioning must be verified in WKWebView (macOS Tauri) and Android WebView on the bench** — where unsupported, positioning silently breaks. |
| `command` | Adopt with deviations — strongest core of the six | `CommandInput`/`CommandItem` are properly reactive (`RwSignal` query, `Memo` visibility, command.rs:246–262, 595–606) with `should_filter=false` + `on_search_change` for server-backed lists — exactly the quick-add/destination-picker shape. `CommandDialog` takes a caller-supplied `id` (no counter bug) and uses Leptos `Portal` (command.rs:212) — content mounts client-side only, fine for a hidden-until-invoked ⌘K. Deviations: parallel vanilla-JS keyboard/filter script fights the reactive path (both write item visibility, command.rs:303–325 vs 622) — discard it and drive keys from Leptos (needed anyway for ⇧⏎ / ⌥⏎ / count entry); ScrollLock. |
| `hover_card` | Adopt with deviations | SSR-safe (`popover="manual"` closed markup). Deviations: counter ID (hover_card.rs:32); same CSS-anchor-positioning platform caveat as popover; hover intent is inline-JS timers (hover_card.rs:103–110) — keep or port. |
| `sheet` | Adopt with deviations | Same skeleton as dialog: SSR-safe closed markup; counter ID (sheet.rs:40); ScrollLock; JS-only open/close. Bottom variant is a fixed `h-[400px]` panel — the mobile card/filter sheet chrome (grabber, drag-to-dismiss, snap heights) is ours on top. |
| `sonner` | Adopt with deviations | Rust side is markup-only (sonner.rs — zero scripts, zero IDs): trivially SSR-safe. Deviations: the actual toast engine is a separate asset pair (`public/app_components/sonner.js` + `sonner.css`) that vendoring the `.rs` does NOT bring along; triggering is declarative (`data-toast-*` on a button, sonner.rs:61–71) — our undo toasts fire programmatically after actions, so we either call the JS engine from Leptos or write the small native toaster (upstream's own unfinished `_sonner_leptos_only_later/` suggests the same conclusion). |

**Bottom line:** nothing here blocks adoption at the markup/styling layer, and `command`'s reactive core is a genuine fit for the app's central interaction. But the interactive layer of dialog/popover/sheet/hover_card is static-site-grade JS that our keyboard-first, programmatically-driven flows will largely replace with Leptos-native state — plan component adoption as "vendor markup + CSS, rewire behavior," not "drop in." Runtime verification of all six lands with the component bench ([specs/ui-component-bench.md](../specs/ui-component-bench.md), draft).
