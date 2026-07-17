mod commands;
mod db;
mod import;
mod ledger;
mod lots;
mod migrations;
mod models;
mod seed;

use rusqlite::Connection;
use std::sync::Mutex;
use tauri::Manager;

/// The one piece of shared, mutable state: a single SQLite connection behind
/// a Mutex. SQLite is single-writer anyway, so this isn't a bottleneck — it's
/// just how a Python/C# dev's mental "global connection" gets typed honestly
/// in Rust: you can share it across threads, but you must say so, and every
/// access takes the lock.
pub struct AppState {
    pub db: Mutex<Connection>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let conn = db::open(&app_data_dir)?;
            app.manage(AppState {
                db: Mutex::new(conn),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_holdings,
            commands::add_ledger_entry,
            commands::add_sell_entry,
            commands::add_trade_entry,
            commands::add_transfer_entry,
            commands::add_gift_out_entry,
            commands::add_income_entry,
            commands::add_fee_burn_entry,
            commands::add_dust_in_entry,
            commands::list_ledger_entries,
            commands::list_lots,
            commands::list_review_flags,
            commands::list_venue_hierarchy,
            commands::import_csv_preview,
            commands::list_staging_rows,
            commands::resolve_staging_row,
            commands::import_csv_commit,
            commands::list_pending_transfers,
            commands::resolve_pending_transfer_pair,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
