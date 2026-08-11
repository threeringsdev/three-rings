// Gives cargo a dependency edge on the migrations directory that
// `sqlx::migrate!("../migrations")` (app/src/db.rs) otherwise lacks: the macro's
// directory-tracking (`tracked_path::path`) is gated behind `sqlx_macros_unstable`,
// a cfg this repo does not set, so without this build script cargo has no reason
// to re-run the macro (and thus re-embed migrations/*.sql) when the directory
// changes — see specs/phase-6-probes/P6-059.md for the root-cause analysis.
//
// Runs on the host for every target that builds `app` (hosted, native/NDK cross,
// wasm32 hydrate) and CI has no DATABASE_URL, so this must be unconditional and
// side-effect-free: one line, no I/O beyond the print itself.
fn main() {
    println!("cargo:rerun-if-changed=../migrations");
}
