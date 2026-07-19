---
name: validate
description: Reproduce the three-rings merge gate (fmt, clippy native+wasm+bench, tests, cargo leptos release build) exactly as CI runs it, with environment-aware exclusions and per-step exit codes. Use before any push, PR, or "is this green?" claim, whenever the user says validate, run the gate, run the checks, or pre-push — and after any multi-file change even if the user doesn't ask. Branch protection auto-merges on green, so this suite is the de facto reviewer.
---

# Validate — reproduce the merge gate

The gate is [.github/workflows/validate.yml](../../../.github/workflows/validate.yml); branch
protection on `main` requires it and auto-merge ships on green, so **a
wrong-but-green change ships itself**. Reproducing it locally before pushing is
the repo's substitute for human review. Run it exactly — a subset proves
nothing.

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
cargo leptos build --release
```

The two `component-bench` clippy lines are part of the gate: the bench is
cfg'd out of every release command, so nothing else ever compiles that code.

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
→ gate green: safe to push
```

On the first failure, stop, show the relevant tail of that command's output,
and fix it before re-running — don't report later steps as green when an
earlier one failed.
