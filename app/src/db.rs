//! Direct Neon access for the **hosted** backend: the shared runtime pool and
//! the migration runner. Behind the `hosted` feature — only the web deployment
//! (the authorization terminus) holds Postgres credentials. The hosted
//! `*Store` impls ([`crate::backend::hosted`]) run all queries through this
//! pool; the native shell reaches data over HTTPS and never links this module.
//!
//! Migrations do **not** run here. They run as a separate deploy step
//! (`server --migrate`, e.g. a Render pre-deploy command) under the
//! owner/migration role, so the long-running server can connect as a
//! non-owner, RLS-subject role that holds no DDL privileges
//! (see specs/data-model.md → Migration plan).

use sqlx::migrate::Migrate;
use sqlx::postgres::PgPoolOptions;
use sqlx::{Connection, PgConnection, PgPool};
use std::collections::HashSet;
use tokio::sync::OnceCell;

static POOL: OnceCell<PgPool> = OnceCell::const_new();

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../migrations");

/// What a `migrate()` call actually did, established by reading the database's
/// applied-migrations table back — not trusted from `MIGRATOR.run`'s bare
/// `Ok(())`, which returns identically whether it applied nine migrations or
/// zero (specs/phase-6-probes/P6-059.md).
///
/// Caveat this type cannot cover on its own: `embedded` is only ever what
/// *this running binary* has compiled in. If the binary itself is stale (a
/// build that silently skipped re-embedding a newly added `.sql` file), every
/// field here is internally consistent — DB and binary agree — while the
/// file on disk is never applied. `app/build.rs` closes that hole on a normal
/// build; `scripts/migrate.sh` closes the residual by independently counting
/// `migrations/*.sql` on disk and comparing it to `embedded`'s count (parsed
/// from the printed report), which this type cannot do from inside the
/// process being asked whether it's stale.
#[derive(Debug, Default)]
pub struct MigrationReport {
    /// Host only (no credentials) of the database this run touched.
    pub host: String,
    /// Versions that were pending before this call and applied during it.
    pub applied: Vec<i64>,
    /// Every distinct version this binary embeds, ascending (a reversible
    /// migration's `.up`/`.down` pair share one version and count once).
    pub embedded: Vec<i64>,
    /// Embedded versions still absent from the database after a run that
    /// returned `Ok` — should always be empty; non-empty means `MIGRATOR::run`
    /// claimed success without actually applying something it embeds.
    /// Defense-in-depth: in practice sqlx's own `validate_applied_migrations`
    /// (inside `MIGRATOR::run`) already refuses to run at all when the DB and
    /// binary disagree, so this rarely has the chance to be observed non-empty.
    pub missing: Vec<i64>,
    /// Versions present in the database's applied-migrations table that this
    /// binary does not embed — drift in the other direction (a stale binary
    /// running against a database a newer one already migrated). Same
    /// defense-in-depth caveat as `missing`: sqlx's own `VersionMissing` check
    /// inside `MIGRATOR::run` is the primary guard for this; see `MigrateFailure`.
    pub unknown: Vec<i64>,
    /// Set when `MIGRATOR::run` itself succeeded — the migration(s) are
    /// committed — but the post-run read-back that verifies it independently
    /// failed (e.g. a transient network blip). `applied`/`missing`/`unknown`
    /// are empty in this case; there is nothing to report but the message.
    pub verify_read_failed: Option<String>,
}

/// A `migrate()` call that did not complete successfully.
#[derive(Debug)]
pub struct MigrateFailure {
    pub error: sqlx::Error,
    /// Host only (no credentials) of the database this run touched.
    pub host: String,
    /// Versions confirmed applied (present after, absent before) despite the
    /// run ultimately failing — sqlx applies each migration in its own
    /// transaction, so a mid-run failure on migration N still leaves any
    /// migrations before it committed and visible here. Best-effort: empty
    /// if the read-back itself also failed, not necessarily if nothing applied.
    pub applied_before_failure: Vec<i64>,
}

/// Host only (no credentials), e.g. `ep-xxxx.region.aws.neon.tech` — parsed
/// the same way `scripts/migrate.sh` extracts it for its own echo line, so a
/// direct `server --migrate` invocation (bypassing the script) still names
/// which branch it touched without ever printing the credential.
fn host_from_url(url: &str) -> String {
    url.split('@')
        .nth(1)
        .and_then(|rest| rest.split(['/', '?']).next())
        .unwrap_or("unknown-host")
        .to_string()
}

/// Sort and dedupe a version list — used both for the embedded set (a
/// reversible migration's `.up`/`.down` pair share one version in
/// `MIGRATOR::iter()`) and for applied-set reads.
fn distinct_sorted(versions: impl IntoIterator<Item = i64>) -> Vec<i64> {
    let mut v: Vec<i64> = versions.into_iter().collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// Read the sorted, distinct list of applied migration versions via a
/// dedicated connection (not the pool): `Migrate::list_applied_migrations` is
/// a trait method on `PgConnection`, and we want a read independent of
/// whatever the pool's single connection is doing. Calls
/// `ensure_migrations_table` first (idempotent `CREATE TABLE IF NOT EXISTS`)
/// since a read taken *before* the very first migration ever runs would
/// otherwise fail — so this "read" can itself create the tracking table.
async fn applied_versions(url: &str) -> Result<Vec<i64>, sqlx::Error> {
    let mut conn = PgConnection::connect(url).await?;
    conn.ensure_migrations_table()
        .await
        .map_err(|e| sqlx::Error::Migrate(Box::new(e)))?;
    let applied = conn
        .list_applied_migrations()
        .await
        .map_err(|e| sqlx::Error::Migrate(Box::new(e)))?
        .into_iter()
        .map(|m| m.version);
    let applied = distinct_sorted(applied);
    conn.close().await?;
    Ok(applied)
}

/// Shared runtime connection pool; connects on first call. Runs **no**
/// migrations — that is the `migrate()` deploy step's job. Requires
/// `DATABASE_URL` (see `.devcontainer/.env.example`).
pub async fn pool() -> Result<&'static PgPool, sqlx::Error> {
    POOL.get_or_try_init(|| async {
        let url = std::env::var("DATABASE_URL")
            .map_err(|_| sqlx::Error::Configuration("DATABASE_URL is not set".into()))?;
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await?;
        Ok(pool)
    })
    .await
}

/// Run pending migrations, then exit. Invoked by the `server --migrate` deploy
/// step (not the serving path), connecting as the owner/migration role via
/// `MIGRATION_DATABASE_URL` (falling back to `DATABASE_URL` for local dev where
/// one credential is used for both). Uses a short-lived one-connection pool.
///
/// Returns a [`MigrationReport`] built from the database's own applied-set,
/// not from `MIGRATOR.run`'s bare `Ok(())` — which is byte-identical whether it
/// applied nine migrations or zero (specs/phase-6-probes/P6-059.md). On
/// failure returns [`MigrateFailure`] rather than a bare `sqlx::Error`, since
/// a mid-run failure can still have committed migrations before it (sqlx
/// applies each one in its own transaction) — the caller should be able to
/// say what *did* land, not just that something went wrong.
///
/// A transient failure on the *post-run verification read* specifically does
/// NOT surface as `Err`: `MIGRATOR::run` already returned `Ok`, so the
/// migration is committed regardless of whether this process can immediately
/// re-read it back. That case comes back as `Ok` with
/// [`MigrationReport::verify_read_failed`] set instead — see the caller
/// (`server`'s `--migrate` arm), which must exit 0 there rather than turn a
/// successful migration into a reported failure.
pub async fn migrate() -> Result<MigrationReport, MigrateFailure> {
    let url = std::env::var("MIGRATION_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .map_err(|_| {
            sqlx::Error::Configuration(
                "neither MIGRATION_DATABASE_URL nor DATABASE_URL is set".into(),
            )
        })
        .map_err(|error| MigrateFailure {
            error,
            host: "unknown-host".to_string(),
            applied_before_failure: Vec::new(),
        })?;
    let host = host_from_url(&url);

    let embedded = distinct_sorted(MIGRATOR.iter().map(|m| m.version));

    let fail = |error: sqlx::Error, applied_before_failure: Vec<i64>| MigrateFailure {
        error,
        host: host.clone(),
        applied_before_failure,
    };

    let before = applied_versions(&url)
        .await
        .map_err(|e| fail(e, Vec::new()))?;

    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .map_err(|e| fail(e, Vec::new()))?;
    let run_result = MIGRATOR.run(&pool).await;
    pool.close().await;

    if let Err(run_err) = run_result {
        // Best-effort: sqlx applies each migration in its own transaction, so
        // migrations before the one that failed are already committed and
        // visible here even though the overall run is an Err. If this
        // read itself fails too, we simply can't say — empty, not "none".
        let applied_before_failure = applied_versions(&url)
            .await
            .map(|after| {
                let before_set: HashSet<i64> = before.iter().copied().collect();
                distinct_sorted(after.into_iter().filter(|v| !before_set.contains(v)))
            })
            .unwrap_or_default();
        return Err(fail(
            sqlx::Error::Migrate(Box::new(run_err)),
            applied_before_failure,
        ));
    }

    // MIGRATOR::run returned Ok — the migration(s), if any, are committed.
    // A failure reading them back here is NOT this run's failure; report it
    // as a successful-but-unverified run rather than a false FAILED.
    let after = match applied_versions(&url).await {
        Ok(after) => after,
        Err(e) => {
            return Ok(MigrationReport {
                host,
                embedded,
                verify_read_failed: Some(e.to_string()),
                ..Default::default()
            });
        }
    };

    let before_set: HashSet<i64> = before.iter().copied().collect();
    let after_set: HashSet<i64> = after.iter().copied().collect();
    let embedded_set: HashSet<i64> = embedded.iter().copied().collect();

    let applied = distinct_sorted(
        after_set
            .difference(&before_set)
            .copied()
            .collect::<Vec<_>>(),
    );

    // Embedded but the DB still doesn't have it after a run that returned Ok —
    // MIGRATOR::run claimed success without actually applying something it
    // embeds. Defense-in-depth: see the field doc on MigrationReport::missing.
    let missing = distinct_sorted(
        embedded_set
            .difference(&after_set)
            .copied()
            .collect::<Vec<_>>(),
    );

    // Applied in the DB but this binary doesn't embed it — a stale binary
    // running against a database a newer one already migrated. Same
    // defense-in-depth caveat as `missing`.
    let unknown = distinct_sorted(
        after_set
            .difference(&embedded_set)
            .copied()
            .collect::<Vec<_>>(),
    );

    Ok(MigrationReport {
        host,
        applied,
        embedded,
        missing,
        unknown,
        verify_read_failed: None,
    })
}
