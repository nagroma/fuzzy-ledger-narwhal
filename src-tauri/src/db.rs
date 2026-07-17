// SQLite connection + schema. STRICT tables, foreign keys on, no DECIMAL type:
// qty and cost_basis_per_unit are stored as TEXT (Decimal's exact string form)
// and parsed back through rust_decimal at the edges. Never REAL.
//
// Schema itself lives in versioned migrations (see migrations.rs) rather than
// an ad hoc execute_batch — see architecture.md for why.

use crate::{migrations, seed};
use rusqlite::Connection;
use std::error::Error;
use std::path::Path;

pub fn open(app_data_dir: &Path) -> Result<Connection, Box<dyn Error>> {
    std::fs::create_dir_all(app_data_dir)?;
    let conn = Connection::open(app_data_dir.join("cryptotracker.sqlite"))?;
    conn.pragma_update(None, "foreign_keys", true)?;
    migrations::run(&conn)?;
    seed::seed_reference_data(&conn)?;
    Ok(conn)
}
