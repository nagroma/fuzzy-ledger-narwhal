// Versioned schema migrations, tracked via SQLite's built-in `PRAGMA
// user_version`. Each migration is a plain .sql file under
// src-tauri/migrations/, embedded at compile time with include_str! (no
// runtime file I/O, and the .sql stays readable/diffable instead of living
// inside a Rust string literal).
//
// Once a migration ships, it is never edited — a schema change is always a
// new numbered file, appended to MIGRATIONS. `run` is idempotent and safe to
// call on every `db::open()`: it only ever applies migrations newer than the
// database's current `user_version`, which is exactly the mechanism that
// brings an *existing* on-disk database (with real data in it) up to date
// when a new release ships — not just a fresh-install concern.

use rusqlite::Connection;
use std::error::Error;

const MIGRATIONS: &[(&str, &str)] = &[(
    "0001_initial_schema",
    include_str!("../migrations/0001_initial_schema.sql"),
)];

pub fn run(conn: &Connection) -> Result<(), Box<dyn Error>> {
    let current_version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

    for (i, (name, sql)) in MIGRATIONS.iter().enumerate() {
        let version = (i + 1) as i64;
        if version <= current_version {
            continue;
        }
        let batch = format!("BEGIN;\n{sql}\nCOMMIT;");
        conn.execute_batch(&batch)
            .map_err(|e| format!("migration {name} failed: {e}"))?;
        conn.pragma_update(None, "user_version", version)?;
    }

    Ok(())
}
