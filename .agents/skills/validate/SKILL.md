---
name: validate
description: Reproduce the three-rings merge gate locally (fmt, clippy native+wasm+bench, tests, cargo leptos release build) with environment-aware exclusions and per-step exit codes. Use to DEBUG a red CI run whose logs don't explain themselves, when the user asks for it by name (validate, run the gate, run the checks), or when CI is unavailable. Do NOT run it as a pre-push ritual — CI runs this same suite on every PR in 2-3 minutes and is the authoritative check.
---

# Validate — reproduce the merge gate locally

The gate is [.github/workflows/validate.yml](../../../.github/workflows/validate.yml); branch
protection on `main` requires it and auto-merge ships on green, so **a
wrong-but-green change ships itself**. Run it exactly — a subset proves nothing.

## When to run this (and when not to)

**CI is the gate. This skill reproduces it; it does not replace it.**
`validate.yml` runs these same eight steps on every PR in **2–3 minutes**, and
a red gate simply doesn't merge — auto-merge fires only on green, so nothing
broken can ship while you weren't looking.

Run this locally when:

- **CI went red and the logs don't explain it** — the primary use. Reproduce
  locally, iterate with a real repro, push the fix.
- The user asks for it by name.
- You want a fast fail on an unusually large or risky change before burning a
  cloud cycle.
- CI is unavailable (offline, Actions down, no remote).

**Do not** run it reflexively before every push, or "to be safe" after a
multi-file change. That was the old contract and it was measured to cost ~53k
tokens a task for an answer CI was about to give anyway (specs/ui-work-loop.md
Findings, 2026-07-25).

A local green is also **not the same green**: CI is linux with Tauri's system
libs, the laptop is macOS, and the devcontainer cannot build `three_rings` at
all. Local agreement is evidence, not proof.

## Environment detection (run first)

The web-dev container deliberately omits Tauri's Linux system libraries, so the
Tauri shell crate (`three_rings`) cannot build there:

- **macOS host** (`uname -s` = Darwin): run the full suite — Xcode supplies the
  Tauri platform bits.
- **The devcontainer** (Linux + `/.dockerenv` present, or
  `pkg-config --exists webkit2gtk-4.1` fails): add
  `--exclude three_rings` to the native clippy and test commands. Everything
  else runs as written.
- **Other Linux with the Tauri deps installed** (CI-like): full suite.

## The suite

Run from the repo root. `mkdir -p target/site/pkg` first — the Tauri build
script reads that directory and fails confusingly without it.

```bash
mkdir -p target/site/pkg
cargo fmt --all -- --check
cargo clippy --workspace --exclude frontend --all-targets -- -D warnings   # add --exclude three_rings in-container
cargo clippy -p frontend --target wasm32-unknown-unknown -- -D warnings
cargo clippy -p app --features native --all-targets -- -D warnings         # native backend: masked by hosted in the workspace line
cargo clippy -p app --features hosted,component-bench --all-targets -- -D warnings
cargo clippy -p app --features hydrate,component-bench --target wasm32-unknown-unknown -- -D warnings
cargo test --workspace --exclude frontend                                  # add --exclude three_rings in-container
CARGO_TARGET_DIR=target/gate cargo leptos build --release                  # own target dir — see below
```

The two `component-bench` clippy lines are part of the gate: the bench is
cfg'd out of every release command, so nothing else ever compiles that code.

The release build gets its **own target dir** (`CARGO_TARGET_DIR=target/gate`).
`site-root` in `Cargo.toml` is the `CARGO_TARGET_DIR/site` marker, so with the env
set the build writes its site to `target/gate/site` instead of `target/site` — the
directory a concurrently-running `cargo leptos watch` serves on :3000. Without the
isolation the release build overwrites `target/site/pkg` with release wasm while
the debug server keeps serving debug SSR, and every page hydration-panics until the
watch is restarted (a source touch does not reliably fix it). CI omits the env var:
there is no watch server there, and keeping the output under `target/` preserves the
rust-cache paths.

## Exit-code discipline

Judge each step by **its own exit code**, never by piped output. A pipeline's
exit code is the *last* command's — `cargo clippy ... | tail` reports tail's
success and silently masks a clippy failure (this exact false-green has
happened in this repo). Run each command bare, or capture `$?` immediately
after it; note that zsh does not word-split unquoted variables, so don't build
commands in shell strings.

Long steps (`cargo leptos build --release` especially) are normal — run them
in the background and wait for completion rather than truncating or skipping.

## Report format

End with a per-step verdict the user can trust at a glance:

```
fmt                       pass
clippy native workspace   pass
clippy frontend wasm      pass
clippy native backend     pass
clippy bench (hosted)     pass
clippy bench (wasm)       pass
test workspace            pass
leptos release build      pass
→ gate green locally (CI's linux run is the authoritative one)
```

On the first failure, stop, show the relevant tail of that command's output,
and fix it before re-running — don't report later steps as green when an
earlier one failed.
