# Three Rings

A cross-platform Magic: The Gathering card collection manager. Track your collection against the full card catalog (~100K cards, routinely updated) on desktop, mobile, and the web.

## Architecture

One Rust codebase, one deployable core. The `app` crate contains the entire application — Leptos UI, Leptos server functions, and the API/data-access layer as an Axum router. That crate ships two ways:

- **Web:** a thin `server` binary hosts the router as an ordinary web app.
- **Desktop/mobile:** Tauri embeds the same router as a Tokio task inside the native process (dynamic port on `127.0.0.1`), and the WebView navigates to it — full SSR and server functions in a single native binary, no sidecar. Pattern per [tauri-leptos-ssr](https://github.com/codeitlikemiley/tauri-leptos-ssr).

All persistent data lives in a single Postgres database on Neon: the shared card catalog and per-user collections, joined server-side. A scheduled ingestion job keeps the catalog current from Scryfall bulk data.

```
                ┌───────────────────────────────┐
                │           app crate           │
                │  Leptos UI + server functions │
                │      + Axum API router        │
                └──────┬─────────────────┬──────┘
              feature: ssr          feature: ssr
                       │                 │
        ┌──────────────▼───┐   ┌─────────▼──────────────┐
        │  server binary   │   │  Tauri shell           │
        │  (hosted web app)│   │  (desktop & mobile)    │
        │                  │   │  embedded Axum + WebView│
        └────────┬─────────┘   └─────────┬──────────────┘
                 │                       │
                 └──────────┬────────────┘
                            │
                  ┌─────────▼─────────┐      ┌───────────────┐
                  │  Postgres (Neon)  │◄─────│ Catalog        │
                  │ catalog + users + │      │ ingestion job  │
                  │    collections    │      │ (Scryfall)     │
                  └───────────────────┘      └───────────────┘
```

### Stack

| Layer | Choice |
|---|---|
| UI | [Leptos](https://www.leptos.dev/) (SSR + hydration) |
| Native shell | [Tauri v2](https://tauri.app/) with embedded Axum server |
| API / server | Axum + sqlx, inside the shared `app` crate |
| Database | Postgres on [Neon](https://neon.com/) |
| Auth | Sessions/JWT at the API layer |
| Catalog ingestion | Scheduled Rust job (Scryfall bulk data) |

### Why

- **Maximal code reuse.** One `app` crate is the whole product; web hosting and native shells are thin wrappers around it. Shared types between client and server eliminate API drift.

### Open consideration

In the Tauri build, server functions execute on the user's machine, so the embedded server must not hold direct Postgres credentials. The data-access layer needs two backends behind one trait: direct sqlx in the hosted deployment, authenticated calls to the hosted API in native builds. Design lands in `specs/`.

## Repository layout

```
├── README.md
├── specs/        # feature specs and todos — see specs/README.md
└── (planned: app/, frontend/, server/, src-tauri/ per the tauri-leptos-ssr layout)
```

## Status

Pre-implementation. Planning lives in `specs/`.
