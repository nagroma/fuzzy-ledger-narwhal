// Tauri commands: the only place Decimal/DateTime (engine types) convert
// to/from String (wire types). Everything below the boundary is
// `lots::Lot`/`ledger::LedgerRow`; everything above it is a models::*Dto.

use chrono::{NaiveDate, TimeZone, Utc};
use rusqlite::Connection;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

use crate::import::csv_parse::{parse_csv, CsvFormat};
use crate::import::staging;
use crate::ledger::{self, EntryType, LedgerError, LedgerRow, ReviewReason};
use crate::lots::LotError;
use crate::models::{
    AddressDto, CommitSummaryDto, Holding, HoldingHierarchyDto, ImportPreviewDto, LedgerEntryDto,
    LotDto, NewDisposalEntry, NewDustInEntry, NewIncomeEntry, NewLedgerEntry, NewTradeEntry,
    NewTransferEntry, ReviewFlagDto, StagingRowDto, VenueHierarchyDto,
};
use crate::AppState;

/// Computed display label for a holding: the venue's name alone when it's
/// the venue's only (default) holding, "Venue — Holding" otherwise. A full
/// Venue/Holding/Address hierarchy view is a later phase; this is just
/// enough to populate a picker.
const HOLDING_DISPLAY_NAME_SQL: &str = "
    CASE WHEN h.is_default_holding = 1 THEN v.name ELSE v.name || ' — ' || h.name END";

#[tauri::command]
pub fn list_holdings(state: tauri::State<AppState>) -> Result<Vec<Holding>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    query_holdings(&conn)
}

#[tauri::command]
pub fn add_ledger_entry(
    state: tauri::State<AppState>,
    entry: NewLedgerEntry,
) -> Result<LedgerEntryDto, String> {
    let qty = parse_positive_decimal(&entry.qty, "Quantity")?;
    let cost_basis_per_unit = parse_nonnegative_decimal(&entry.cost_basis_per_unit, "Cost basis")?;
    let event_at = parse_event_date(&entry.acquired_at, "Acquired date")?;
    let asset = normalize_asset(&entry.asset)?;

    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let created_at = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO ledger_entries
            (holding_id, entry_type, asset, qty, cost_basis_per_unit, event_at, entry_source, created_at)
         VALUES (?1, 'BUY', ?2, ?3, ?4, ?5, 'MANUAL', ?6)",
        rusqlite::params![
            entry.holding_id,
            asset,
            qty.to_string(),
            cost_basis_per_unit.to_string(),
            event_at,
            created_at,
        ],
    )
    .map_err(|e| e.to_string())?;

    let id = conn.last_insert_rowid();
    query_ledger_entry(&conn, id)
}

#[tauri::command]
pub fn add_sell_entry(
    state: tauri::State<AppState>,
    entry: NewDisposalEntry,
) -> Result<LedgerEntryDto, String> {
    insert_disposal(state, entry, "SELL")
}

#[tauri::command]
pub fn add_gift_out_entry(
    state: tauri::State<AppState>,
    entry: NewDisposalEntry,
) -> Result<LedgerEntryDto, String> {
    insert_disposal(state, entry, "GIFT_OUT")
}

#[tauri::command]
pub fn add_fee_burn_entry(
    state: tauri::State<AppState>,
    entry: NewDisposalEntry,
) -> Result<LedgerEntryDto, String> {
    insert_disposal(state, entry, "FEE_BURN")
}

fn insert_disposal(
    state: tauri::State<AppState>,
    entry: NewDisposalEntry,
    entry_type: &str,
) -> Result<LedgerEntryDto, String> {
    let qty = parse_positive_decimal(&entry.qty, "Quantity")?;
    let proceeds_total = parse_nonnegative_decimal(&entry.proceeds_total, "Proceeds")?;
    let event_at = parse_event_date(&entry.event_at, "Date")?;
    let asset = normalize_asset(&entry.asset)?;

    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let created_at = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO ledger_entries
            (holding_id, entry_type, asset, qty, proceeds_total, event_at, entry_source, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'MANUAL', ?7)",
        rusqlite::params![
            entry.holding_id,
            entry_type,
            asset,
            qty.to_string(),
            proceeds_total.to_string(),
            event_at,
            created_at,
        ],
    )
    .map_err(|e| e.to_string())?;

    let id = conn.last_insert_rowid();
    query_ledger_entry(&conn, id)
}

#[tauri::command]
pub fn add_income_entry(
    state: tauri::State<AppState>,
    entry: NewIncomeEntry,
) -> Result<LedgerEntryDto, String> {
    let qty = parse_positive_decimal(&entry.qty, "Quantity")?;
    let cost_basis_per_unit = parse_nonnegative_decimal(&entry.cost_basis_per_unit, "Cost basis")?;
    let event_at = parse_event_date(&entry.event_at, "Date")?;
    let asset = normalize_asset(&entry.asset)?;

    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let created_at = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO ledger_entries
            (holding_id, entry_type, asset, qty, cost_basis_per_unit, event_at, entry_source, created_at)
         VALUES (?1, 'INCOME', ?2, ?3, ?4, ?5, 'MANUAL', ?6)",
        rusqlite::params![
            entry.holding_id,
            asset,
            qty.to_string(),
            cost_basis_per_unit.to_string(),
            event_at,
            created_at,
        ],
    )
    .map_err(|e| e.to_string())?;

    let id = conn.last_insert_rowid();
    query_ledger_entry(&conn, id)
}

#[tauri::command]
pub fn add_dust_in_entry(
    state: tauri::State<AppState>,
    entry: NewDustInEntry,
) -> Result<LedgerEntryDto, String> {
    let qty = parse_positive_decimal(&entry.qty, "Quantity")?;
    let event_at = parse_event_date(&entry.event_at, "Date")?;
    let asset = normalize_asset(&entry.asset)?;

    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let created_at = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO ledger_entries
            (holding_id, entry_type, asset, qty, cost_basis_per_unit, event_at, entry_source, created_at)
         VALUES (?1, 'DUST_IN', ?2, ?3, '0', ?4, 'MANUAL', ?5)",
        rusqlite::params![entry.holding_id, asset, qty.to_string(), event_at, created_at],
    )
    .map_err(|e| e.to_string())?;

    let id = conn.last_insert_rowid();
    query_ledger_entry(&conn, id)
}

/// A crypto-to-crypto trade: two rows, one moment, sharing a `trade_id`.
/// TRADE_OUT's proceeds and TRADE_IN's total cost basis are both `fmv_total`
/// — the fair-market-value of what came back is definitionally the basis of
/// what was acquired.
#[tauri::command]
pub fn add_trade_entry(
    state: tauri::State<AppState>,
    entry: NewTradeEntry,
) -> Result<Vec<LedgerEntryDto>, String> {
    let qty_out = parse_positive_decimal(&entry.qty_out, "Quantity given up")?;
    let qty_in = parse_positive_decimal(&entry.qty_in, "Quantity received")?;
    let fmv_total = parse_nonnegative_decimal(&entry.fmv_total, "Fair market value")?;
    let event_at = parse_event_date(&entry.event_at, "Date")?;
    let asset_out = normalize_asset(&entry.asset_out)?;
    let asset_in = normalize_asset(&entry.asset_in)?;

    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let created_at = Utc::now().to_rfc3339();
    let trade_id = uuid::Uuid::new_v4().to_string();

    tx.execute(
        "INSERT INTO ledger_entries
            (holding_id, entry_type, asset, qty, proceeds_total, trade_id, event_at, entry_source, created_at)
         VALUES (?1, 'TRADE_OUT', ?2, ?3, ?4, ?5, ?6, 'MANUAL', ?7)",
        rusqlite::params![
            entry.holding_id,
            asset_out,
            qty_out.to_string(),
            fmv_total.to_string(),
            trade_id,
            event_at,
            created_at,
        ],
    )
    .map_err(|e| e.to_string())?;
    let out_id = tx.last_insert_rowid();

    let cost_basis_per_unit = fmv_total / qty_in;

    tx.execute(
        "INSERT INTO ledger_entries
            (holding_id, entry_type, asset, qty, cost_basis_per_unit, trade_id, event_at, entry_source, created_at)
         VALUES (?1, 'TRADE_IN', ?2, ?3, ?4, ?5, ?6, 'MANUAL', ?7)",
        rusqlite::params![
            entry.holding_id,
            asset_in,
            qty_in.to_string(),
            cost_basis_per_unit.to_string(),
            trade_id,
            event_at,
            created_at,
        ],
    )
    .map_err(|e| e.to_string())?;
    let in_id = tx.last_insert_rowid();

    tx.commit().map_err(|e| e.to_string())?;
    Ok(vec![
        query_ledger_entry(&conn, out_id)?,
        query_ledger_entry(&conn, in_id)?,
    ])
}

/// A transfer between two of the end user's own holdings: two rows, one moment,
/// sharing a `transfer_id`. Non-taxable — `ledger::fold_ledger` is what
/// actually moves basis/acquired_at from the source lot fragments to the
/// new destination lot; this command just records the pair.
#[tauri::command]
pub fn add_transfer_entry(
    state: tauri::State<AppState>,
    entry: NewTransferEntry,
) -> Result<Vec<LedgerEntryDto>, String> {
    let qty = parse_positive_decimal(&entry.qty, "Quantity")?;
    let event_at = parse_event_date(&entry.event_at, "Date")?;
    let asset = normalize_asset(&entry.asset)?;
    if entry.from_holding_id == entry.to_holding_id {
        return Err("Transfer source and destination holdings must differ".to_string());
    }

    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let created_at = Utc::now().to_rfc3339();
    let transfer_id = uuid::Uuid::new_v4().to_string();

    tx.execute(
        "INSERT INTO ledger_entries
            (holding_id, entry_type, asset, qty, transfer_id, event_at, entry_source, created_at)
         VALUES (?1, 'TRANSFER_OUT', ?2, ?3, ?4, ?5, 'MANUAL', ?6)",
        rusqlite::params![
            entry.from_holding_id,
            asset,
            qty.to_string(),
            transfer_id,
            event_at,
            created_at,
        ],
    )
    .map_err(|e| e.to_string())?;
    let out_id = tx.last_insert_rowid();

    tx.execute(
        "INSERT INTO ledger_entries
            (holding_id, entry_type, asset, qty, transfer_id, event_at, entry_source, created_at)
         VALUES (?1, 'TRANSFER_IN', ?2, ?3, ?4, ?5, 'MANUAL', ?6)",
        rusqlite::params![
            entry.to_holding_id,
            asset,
            qty.to_string(),
            transfer_id,
            event_at,
            created_at,
        ],
    )
    .map_err(|e| e.to_string())?;
    let in_id = tx.last_insert_rowid();

    tx.commit().map_err(|e| e.to_string())?;
    Ok(vec![
        query_ledger_entry(&conn, out_id)?,
        query_ledger_entry(&conn, in_id)?,
    ])
}

#[tauri::command]
pub fn list_ledger_entries(state: tauri::State<AppState>) -> Result<Vec<LedgerEntryDto>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let sql = format!(
        "SELECT e.id, e.holding_id, {HOLDING_DISPLAY_NAME_SQL}, e.entry_type, e.asset, e.qty,
                e.cost_basis_per_unit, e.proceeds_total, e.trade_id, e.transfer_id,
                e.event_at, e.created_at
         FROM ledger_entries e
         JOIN holdings h ON h.id = e.holding_id
         JOIN venues v ON v.id = h.venue_id
         ORDER BY e.event_at ASC, e.id ASC"
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], row_to_ledger_entry)
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_lots(state: tauri::State<AppState>) -> Result<Vec<LotDto>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let holdings = query_holdings(&conn)?;
    let result = fold_whole_ledger(&conn)?;

    Ok(result
        .lots
        .into_iter()
        .filter(|lot| lot.qty_remaining > Decimal::ZERO)
        .map(|lot| LotDto {
            lot_id: lot.id,
            holding_id: lot.holding_id,
            holding_name: holdings
                .iter()
                .find(|h| h.id == lot.holding_id)
                .map(|h| h.display_name.clone())
                .unwrap_or_else(|| "Unknown holding".to_string()),
            asset: lot.asset,
            qty_remaining: lot.qty_remaining.to_string(),
            cost_basis_per_unit: lot.cost_basis_per_unit.to_string(),
            acquired_at: lot.acquired_at.to_rfc3339(),
        })
        .collect())
}

#[tauri::command]
pub fn list_review_flags(state: tauri::State<AppState>) -> Result<Vec<ReviewFlagDto>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let result = fold_whole_ledger(&conn)?;
    Ok(result
        .flags
        .into_iter()
        .map(|f| ReviewFlagDto {
            entry_id: f.entry_id,
            reason: match f.reason {
                ReviewReason::CrossEntityTransfer => "CROSS_ENTITY_TRANSFER".to_string(),
                ReviewReason::FeeBurnUnsettled => "FEE_BURN_UNSETTLED".to_string(),
                ReviewReason::DustIncome => "DUST_INCOME".to_string(),
            },
        })
        .collect())
}

#[tauri::command]
pub fn list_venue_hierarchy(state: tauri::State<AppState>) -> Result<Vec<VenueHierarchyDto>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;

    let mut venue_stmt = conn
        .prepare("SELECT id, name, venue_type FROM venues ORDER BY name ASC")
        .map_err(|e| e.to_string())?;
    let venues: Vec<(i64, String, String)> = venue_stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;

    let mut holding_stmt = conn
        .prepare(
            "SELECT h.id, h.name, e.name, h.account_identifier, h.is_default_holding
             FROM holdings h JOIN entities e ON e.id = h.entity_id
             WHERE h.venue_id = ?1
             ORDER BY h.name ASC",
        )
        .map_err(|e| e.to_string())?;
    let mut address_stmt = conn
        .prepare(
            "SELECT id, asset, address, label FROM addresses
             WHERE holding_id = ?1 ORDER BY asset ASC",
        )
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for (venue_id, venue_name, venue_type) in venues {
        let holdings: Vec<(i64, String, String, Option<String>, bool)> = holding_stmt
            .query_map([venue_id], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get::<_, i64>(4)? == 1,
                ))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;

        let mut holding_dtos = Vec::new();
        for (holding_id, holding_name, entity_name, account_identifier, is_default_holding) in holdings {
            let addresses: Vec<AddressDto> = address_stmt
                .query_map([holding_id], |row| {
                    Ok(AddressDto {
                        id: row.get(0)?,
                        asset: row.get(1)?,
                        address: row.get(2)?,
                        label: row.get(3)?,
                    })
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<_, _>>()
                .map_err(|e| e.to_string())?;

            holding_dtos.push(HoldingHierarchyDto {
                holding_id,
                holding_name,
                entity_name,
                account_identifier,
                is_default_holding,
                addresses,
            });
        }

        result.push(VenueHierarchyDto {
            venue_id,
            venue_name,
            venue_type,
            holdings: holding_dtos,
        });
    }

    Ok(result)
}

/// Parse a CSV file and stage every row for review — nothing is written to
/// `ledger_entries`/`cash_activity_entries` yet. `format` is `"DWP_ANCHORAGE"`
/// or `"GENERIC_PLACEHOLDER"` (the latter is a stand-in for venues without a
/// confirmed real export format).
#[tauri::command]
pub fn import_csv_preview(
    state: tauri::State<AppState>,
    file_path: String,
    venue_id: i64,
    holding_id: i64,
    format: String,
) -> Result<ImportPreviewDto, String> {
    let format = match format.as_str() {
        "DWP_ANCHORAGE" => CsvFormat::DwpAnchorage,
        "GENERIC_PLACEHOLDER" => CsvFormat::GenericPlaceholder,
        other => return Err(format!("Unknown CSV format: {other}")),
    };
    let content = std::fs::read_to_string(&file_path).map_err(|e| e.to_string())?;
    let (rows, errors) = parse_csv(format, &content);

    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let source_filename = std::path::Path::new(&file_path)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or(file_path.clone());
    let batch_id = staging::create_import_batch(&conn, venue_id, &source_filename, rows.len())?;
    staging::stage_parsed_rows(&conn, batch_id, holding_id, &rows)?;

    let staged = staging::list_staging_rows(&conn, batch_id)?;
    let new_count = staged.iter().filter(|r| r.classification == "NEW").count() as i64;
    let matched_existing_count = staged.iter().filter(|r| r.classification == "MATCHED_EXISTING").count() as i64;
    let near_duplicate_count = staged.iter().filter(|r| r.classification == "NEAR_DUPLICATE").count() as i64;
    let pending_transfer_count = staged.iter().filter(|r| r.classification == "PENDING_TRANSFER").count() as i64;
    let cash_activity_count = staged.iter().filter(|r| r.target_table == "CASH_ACTIVITY_ENTRIES").count() as i64;

    Ok(ImportPreviewDto {
        batch_id,
        total_rows: rows.len() as i64,
        new_count,
        matched_existing_count,
        near_duplicate_count,
        pending_transfer_count,
        cash_activity_count,
        parse_errors: errors.into_iter().map(|e| format!("row {}: {}", e.row_number, e.message)).collect(),
    })
}

#[tauri::command]
pub fn list_staging_rows(state: tauri::State<AppState>, batch_id: i64) -> Result<Vec<StagingRowDto>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    Ok(staging::list_staging_rows(&conn, batch_id)?.into_iter().map(staging_row_to_dto).collect())
}

#[tauri::command]
pub fn resolve_staging_row(state: tauri::State<AppState>, row_id: i64, decision: String) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    staging::resolve_staging_row(&conn, row_id, &decision)
}

#[tauri::command]
pub fn import_csv_commit(state: tauri::State<AppState>, batch_id: i64) -> Result<CommitSummaryDto, String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    let summary = staging::commit_batch(&mut conn, batch_id)?;
    Ok(CommitSummaryDto {
        ledger_entries_committed: summary.ledger_entries_committed,
        cash_activity_entries_committed: summary.cash_activity_entries_committed,
        transfers_auto_resolved: summary.transfers_auto_resolved,
    })
}

/// Every still-unresolved transfer leg across every import, so far — flagged
/// in the UI until matched (automatically, on the next commit) or manually
/// paired with `resolve_pending_transfer_pair`.
#[tauri::command]
pub fn list_pending_transfers(state: tauri::State<AppState>) -> Result<Vec<StagingRowDto>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    Ok(staging::list_pending_transfers(&conn)?.into_iter().map(staging_row_to_dto).collect())
}

#[tauri::command]
pub fn resolve_pending_transfer_pair(
    state: tauri::State<AppState>,
    out_staging_id: i64,
    in_staging_id: i64,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    staging::resolve_pending_transfer_pair(&conn, out_staging_id, in_staging_id)
}

fn staging_row_to_dto(row: staging::StagingRow) -> StagingRowDto {
    StagingRowDto {
        id: row.id,
        import_batch_id: row.import_batch_id,
        target_table: row.target_table,
        entry_type: row.parsed_entry_type,
        asset: row.parsed_asset,
        qty: row.parsed_qty,
        cost_basis_per_unit: row.parsed_cost_basis_per_unit,
        proceeds_total: row.parsed_proceeds_total,
        event_at: row.parsed_timestamp,
        classification: row.classification,
        decision: row.decision,
        transfer_leg: row.transfer_leg,
    }
}

fn query_holdings(conn: &Connection) -> Result<Vec<Holding>, String> {
    let sql = format!(
        "SELECT h.id, {HOLDING_DISPLAY_NAME_SQL}
         FROM holdings h
         JOIN venues v ON v.id = h.venue_id
         ORDER BY v.name ASC, h.name ASC"
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(Holding {
                id: row.get(0)?,
                display_name: row.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

fn query_ledger_entry(conn: &Connection, id: i64) -> Result<LedgerEntryDto, String> {
    let sql = format!(
        "SELECT e.id, e.holding_id, {HOLDING_DISPLAY_NAME_SQL}, e.entry_type, e.asset, e.qty,
                e.cost_basis_per_unit, e.proceeds_total, e.trade_id, e.transfer_id,
                e.event_at, e.created_at
         FROM ledger_entries e
         JOIN holdings h ON h.id = e.holding_id
         JOIN venues v ON v.id = h.venue_id
         WHERE e.id = ?1"
    );
    conn.query_row(&sql, [id], row_to_ledger_entry)
        .map_err(|e| e.to_string())
}

fn row_to_ledger_entry(row: &rusqlite::Row) -> rusqlite::Result<LedgerEntryDto> {
    Ok(LedgerEntryDto {
        id: row.get(0)?,
        holding_id: row.get(1)?,
        holding_name: row.get(2)?,
        entry_type: row.get(3)?,
        asset: row.get(4)?,
        qty: row.get(5)?,
        cost_basis_per_unit: row.get(6)?,
        proceeds_total: row.get(7)?,
        trade_id: row.get(8)?,
        transfer_id: row.get(9)?,
        event_at: row.get(10)?,
        created_at: row.get(11)?,
    })
}

/// Load the entire ledger and fold it through `ledger::fold_ledger`. Derived,
/// not stored: every call rebuilds current lots/flags fresh. Realistic
/// volume (~1.3K transactions/2yr, tens of thousands at most per the plan)
/// makes recomputing on every read trivial — revisit only if profiling ever
/// says otherwise.
fn fold_whole_ledger(conn: &Connection) -> Result<ledger::FoldResult, String> {
    let holding_entity = query_holding_entity_map(conn)?;
    let rows = load_ledger_rows(conn)?;
    ledger::fold_ledger(&rows, &holding_entity).map_err(ledger_error_to_string)
}

fn ledger_error_to_string(e: LedgerError) -> String {
    match e {
        LedgerError::Lot(LotError::InsufficientLots { asset, short_by }) => {
            format!("Not enough {asset} in this holding to dispose (short by {short_by})")
        }
        LedgerError::UnpairedTransfer { transfer_id } => {
            format!("Transfer {transfer_id} has only one leg recorded — the other side is missing")
        }
    }
}

fn query_holding_entity_map(conn: &Connection) -> Result<HashMap<i64, i64>, String> {
    let mut stmt = conn
        .prepare("SELECT id, entity_id FROM holdings")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<HashMap<_, _>, _>>()
        .map_err(|e| e.to_string())
}

/// Load every ledger row, converted into the engine's native types
/// (Decimal/DateTime), ordered the way `dispose_fifo`'s FIFO relies on.
fn load_ledger_rows(conn: &Connection) -> Result<Vec<LedgerRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, holding_id, entry_type, asset, qty, cost_basis_per_unit,
                    proceeds_total, transfer_id, event_at
             FROM ledger_entries
             ORDER BY event_at ASC, id ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, String>(8)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for row in rows {
        let (id, holding_id, entry_type, asset, qty, cost_basis_per_unit, proceeds_total, transfer_id, event_at) =
            row.map_err(|e| e.to_string())?;
        out.push(LedgerRow {
            id,
            holding_id,
            entry_type: parse_entry_type(&entry_type)?,
            asset,
            qty: Decimal::from_str(&qty).map_err(|e| e.to_string())?,
            cost_basis_per_unit: cost_basis_per_unit
                .map(|s| Decimal::from_str(&s))
                .transpose()
                .map_err(|e| e.to_string())?,
            proceeds_total: proceeds_total
                .map(|s| Decimal::from_str(&s))
                .transpose()
                .map_err(|e| e.to_string())?,
            transfer_id,
            event_at: chrono::DateTime::parse_from_rfc3339(&event_at)
                .map_err(|e| e.to_string())?
                .with_timezone(&Utc),
        });
    }
    Ok(out)
}

fn parse_entry_type(s: &str) -> Result<EntryType, String> {
    match s {
        "BUY" => Ok(EntryType::Buy),
        "SELL" => Ok(EntryType::Sell),
        "TRADE_IN" => Ok(EntryType::TradeIn),
        "TRADE_OUT" => Ok(EntryType::TradeOut),
        "TRANSFER_IN" => Ok(EntryType::TransferIn),
        "TRANSFER_OUT" => Ok(EntryType::TransferOut),
        "GIFT_OUT" => Ok(EntryType::GiftOut),
        "INCOME" => Ok(EntryType::Income),
        "FEE_BURN" => Ok(EntryType::FeeBurn),
        "DUST_IN" => Ok(EntryType::DustIn),
        other => Err(format!("Unknown entry_type in database: {other}")),
    }
}

fn parse_positive_decimal(raw: &str, field: &str) -> Result<Decimal, String> {
    let value = Decimal::from_str(raw.trim()).map_err(|_| format!("{field} must be a number"))?;
    if value <= Decimal::ZERO {
        return Err(format!("{field} must be greater than zero"));
    }
    Ok(value)
}

fn parse_nonnegative_decimal(raw: &str, field: &str) -> Result<Decimal, String> {
    let value = Decimal::from_str(raw.trim()).map_err(|_| format!("{field} must be a number"))?;
    if value < Decimal::ZERO {
        return Err(format!("{field} can't be negative"));
    }
    Ok(value)
}

fn parse_event_date(raw: &str, field: &str) -> Result<String, String> {
    let date = NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d")
        .map_err(|_| format!("{field} must be YYYY-MM-DD"))?;
    Ok(Utc
        .from_utc_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
        .to_rfc3339())
}

fn normalize_asset(raw: &str) -> Result<String, String> {
    let asset = raw.trim().to_uppercase();
    if asset.is_empty() {
        return Err("Asset can't be empty".to_string());
    }
    Ok(asset)
}
