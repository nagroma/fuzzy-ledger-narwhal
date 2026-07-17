// rusqlite CRUD for the import reconciliation flow: stage parsed rows,
// classify them against what's already in the ledger, let the user decide,
// then commit. This is the one file in `import/` that touches the database —
// csv_parse.rs and dedup.rs stay pure and independently testable.

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension};
use rust_decimal::Decimal;
use std::str::FromStr;

use super::csv_parse::{ParsedRow, TransferLeg};
use super::dedup::hash_ledger_row;

#[derive(Debug, Clone)]
pub struct StagingRow {
    pub id: i64,
    pub import_batch_id: i64,
    pub target_table: String,
    pub parsed_entry_type: Option<String>,
    pub parsed_asset: Option<String>,
    pub parsed_qty: Option<String>,
    pub parsed_cost_basis_per_unit: Option<String>,
    pub parsed_proceeds_total: Option<String>,
    pub parsed_timestamp: Option<String>,
    pub classification: String,
    pub decision: Option<String>,
    pub transfer_leg: Option<String>,
    pub paired_staging_row_id: Option<i64>,
}

pub fn create_import_batch(
    conn: &Connection,
    venue_id: i64,
    source_filename: &str,
    row_count: usize,
) -> Result<i64, String> {
    let imported_at = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO import_batches (venue_id, source_filename, imported_at, row_count)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![venue_id, source_filename, imported_at, row_count as i64],
    )
    .map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

/// Stage every parsed row (skipping `ParsedRow::Ignored` entirely — those
/// never become a staging row at all). Ledger candidates get classified
/// against what's already in `ledger_entries`: an exact dedup-hash match is
/// `MATCHED_EXISTING`; same holding+asset+date with a different quantity is
/// `NEAR_DUPLICATE` (always surfaced, never auto-resolved); otherwise `NEW`.
pub fn stage_parsed_rows(
    conn: &Connection,
    batch_id: i64,
    holding_id: i64,
    rows: &[ParsedRow],
) -> Result<(), String> {
    for row in rows {
        match row {
            ParsedRow::Ignored => continue,
            ParsedRow::LedgerCandidate {
                entry_type,
                asset,
                qty,
                cost_basis_per_unit,
                proceeds_total,
                event_at,
            } => {
                let dedup_hash = hash_ledger_row(holding_id, entry_type, asset, *qty, event_at);
                let existing_exact: Option<i64> = conn
                    .query_row(
                        "SELECT id FROM ledger_entries WHERE dedup_hash = ?1",
                        [&dedup_hash],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(|e| e.to_string())?;

                let classification = if existing_exact.is_some() {
                    "MATCHED_EXISTING"
                } else {
                    let near_dup_count: i64 = conn
                        .query_row(
                            "SELECT COUNT(*) FROM ledger_entries
                             WHERE holding_id = ?1 AND asset = ?2 AND entry_type = ?3 AND event_at = ?4",
                            rusqlite::params![holding_id, asset, entry_type, event_at],
                            |r| r.get(0),
                        )
                        .map_err(|e| e.to_string())?;
                    if near_dup_count > 0 { "NEAR_DUPLICATE" } else { "NEW" }
                };

                conn.execute(
                    "INSERT INTO import_staging_rows
                        (import_batch_id, raw_row_json, target_table, parsed_holding_id,
                         parsed_entry_type, parsed_asset, parsed_qty, parsed_cost_basis_per_unit,
                         parsed_proceeds_total, parsed_timestamp, computed_dedup_hash, classification)
                     VALUES (?1, ?2, 'LEDGER_ENTRIES', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    rusqlite::params![
                        batch_id,
                        format!("{row:?}"),
                        holding_id,
                        entry_type,
                        asset,
                        qty.to_string(),
                        cost_basis_per_unit.map(|d| d.to_string()),
                        proceeds_total.map(|d| d.to_string()),
                        event_at,
                        dedup_hash,
                        classification,
                    ],
                )
                .map_err(|e| e.to_string())?;
            }
            ParsedRow::CashActivityCandidate {
                kind,
                currency,
                amount,
                event_at,
            } => {
                conn.execute(
                    "INSERT INTO import_staging_rows
                        (import_batch_id, raw_row_json, target_table, parsed_holding_id,
                         parsed_entry_type, parsed_asset, parsed_proceeds_total, parsed_timestamp,
                         classification)
                     VALUES (?1, ?2, 'CASH_ACTIVITY_ENTRIES', ?3, ?4, ?5, ?6, ?7, 'NEW')",
                    rusqlite::params![
                        batch_id,
                        format!("{row:?}"),
                        holding_id,
                        kind,
                        currency,
                        amount.to_string(),
                        event_at,
                    ],
                )
                .map_err(|e| e.to_string())?;
            }
            ParsedRow::PendingTransfer { leg, asset, qty, event_at } => {
                let leg_str = match leg {
                    TransferLeg::Out => "OUT",
                    TransferLeg::In => "IN",
                };
                conn.execute(
                    "INSERT INTO import_staging_rows
                        (import_batch_id, raw_row_json, target_table, parsed_holding_id,
                         parsed_asset, parsed_qty, parsed_timestamp, classification, transfer_leg)
                     VALUES (?1, ?2, 'LEDGER_ENTRIES', ?3, ?4, ?5, ?6, 'PENDING_TRANSFER', ?7)",
                    rusqlite::params![
                        batch_id,
                        format!("{row:?}"),
                        holding_id,
                        asset,
                        qty.to_string(),
                        event_at,
                        leg_str,
                    ],
                )
                .map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

pub fn list_staging_rows(conn: &Connection, batch_id: i64) -> Result<Vec<StagingRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, import_batch_id, target_table, parsed_entry_type, parsed_asset, parsed_qty,
                    parsed_cost_basis_per_unit, parsed_proceeds_total, parsed_timestamp,
                    classification, decision, transfer_leg, paired_staging_row_id
             FROM import_staging_rows WHERE import_batch_id = ?1 ORDER BY id ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([batch_id], row_to_staging_row)
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

/// Every still-unresolved pending transfer leg, across every import batch —
/// not scoped to one file, since the matching inbound/outbound leg may have
/// arrived in a completely different venue's export.
pub fn list_pending_transfers(conn: &Connection) -> Result<Vec<StagingRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, import_batch_id, target_table, parsed_entry_type, parsed_asset, parsed_qty,
                    parsed_cost_basis_per_unit, parsed_proceeds_total, parsed_timestamp,
                    classification, decision, transfer_leg, paired_staging_row_id
             FROM import_staging_rows
             WHERE classification = 'PENDING_TRANSFER' AND decision IS NULL
             ORDER BY parsed_timestamp ASC, id ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], row_to_staging_row)
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

fn row_to_staging_row(row: &rusqlite::Row) -> rusqlite::Result<StagingRow> {
    Ok(StagingRow {
        id: row.get(0)?,
        import_batch_id: row.get(1)?,
        target_table: row.get(2)?,
        parsed_entry_type: row.get(3)?,
        parsed_asset: row.get(4)?,
        parsed_qty: row.get(5)?,
        parsed_cost_basis_per_unit: row.get(6)?,
        parsed_proceeds_total: row.get(7)?,
        parsed_timestamp: row.get(8)?,
        classification: row.get(9)?,
        decision: row.get(10)?,
        transfer_leg: row.get(11)?,
        paired_staging_row_id: row.get(12)?,
    })
}

pub fn resolve_staging_row(conn: &Connection, row_id: i64, decision: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE import_staging_rows SET decision = ?1 WHERE id = ?2",
        rusqlite::params![decision, row_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Commit every ACCEPTed/TREAT_AS_NEW row in this batch into its target
/// table, then try to auto-resolve any pending transfer legs this batch
/// introduced against every other still-unresolved leg (any asset/qty exact
/// match, opposite direction, regardless of which batch or venue it came
/// from). Anything that doesn't find a match stays in staging, surfaced by
/// `list_pending_transfers` — never silently written to the ledger, never
/// silently dropped.
pub fn commit_batch(conn: &mut Connection, batch_id: i64) -> Result<CommitSummary, String> {
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let mut ledger_committed = 0;
    let mut cash_committed = 0;

    {
        let mut stmt = tx
            .prepare(
                "SELECT id, target_table, parsed_holding_id, parsed_entry_type, parsed_asset,
                        parsed_qty, parsed_cost_basis_per_unit, parsed_proceeds_total,
                        parsed_timestamp, computed_dedup_hash
                 FROM import_staging_rows
                 WHERE import_batch_id = ?1 AND decision IN ('ACCEPT','TREAT_AS_NEW')",
            )
            .map_err(|e| e.to_string())?;
        let candidates: Vec<(i64, String, i64, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>)> = stmt
            .query_map([batch_id], |r| {
                Ok((
                    r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?,
                    r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;

        for (staging_id, target_table, holding_id, entry_type, asset, qty, cost_basis, proceeds, event_at, dedup_hash) in candidates {
            if target_table == "LEDGER_ENTRIES" {
                let created_at = Utc::now().to_rfc3339();
                tx.execute(
                    "INSERT INTO ledger_entries
                        (holding_id, entry_type, asset, qty, cost_basis_per_unit, proceeds_total,
                         event_at, entry_source, import_batch_id, dedup_hash, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'CSV_IMPORT', ?8, ?9, ?10)",
                    rusqlite::params![
                        holding_id, entry_type, asset, qty, cost_basis, proceeds,
                        event_at, batch_id, dedup_hash, created_at,
                    ],
                )
                .map_err(|e| e.to_string())?;
                let new_id = tx.last_insert_rowid();
                tx.execute(
                    "UPDATE import_staging_rows SET resolved_ledger_entry_id = ?1 WHERE id = ?2",
                    rusqlite::params![new_id, staging_id],
                )
                .map_err(|e| e.to_string())?;
                ledger_committed += 1;
            } else {
                let created_at = Utc::now().to_rfc3339();
                tx.execute(
                    "INSERT INTO cash_activity_entries
                        (holding_id, kind, currency, amount, event_at, entry_source, import_batch_id, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, 'CSV_IMPORT', ?6, ?7)",
                    rusqlite::params![holding_id, entry_type, asset, proceeds, event_at, batch_id, created_at],
                )
                .map_err(|e| e.to_string())?;
                cash_committed += 1;
            }
        }
    }

    let auto_resolved = auto_resolve_pending_transfers(&tx)?;

    tx.commit().map_err(|e| e.to_string())?;
    Ok(CommitSummary {
        ledger_entries_committed: ledger_committed,
        cash_activity_entries_committed: cash_committed,
        transfers_auto_resolved: auto_resolved,
    })
}

pub struct CommitSummary {
    pub ledger_entries_committed: i64,
    pub cash_activity_entries_committed: i64,
    pub transfers_auto_resolved: i64,
}

/// Every unresolved `OUT` leg is checked against every unresolved `IN` leg
/// (any batch) for an exact asset+qty match. Matches are resolved eagerly,
/// oldest first; anything left over stays pending.
fn auto_resolve_pending_transfers(tx: &rusqlite::Transaction) -> Result<i64, String> {
    let mut resolved = 0;
    loop {
        let pair: Option<(i64, i64)> = tx
            .query_row(
                "SELECT o.id, i.id
                 FROM import_staging_rows o
                 JOIN import_staging_rows i
                   ON i.classification = 'PENDING_TRANSFER' AND i.decision IS NULL
                  AND i.transfer_leg = 'IN'
                  AND i.parsed_asset = o.parsed_asset AND i.parsed_qty = o.parsed_qty
                 WHERE o.classification = 'PENDING_TRANSFER' AND o.decision IS NULL
                   AND o.transfer_leg = 'OUT'
                 ORDER BY o.parsed_timestamp ASC
                 LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;

        let Some((out_id, in_id)) = pair else { break };
        resolve_pending_transfer_pair(tx, out_id, in_id)?;
        resolved += 1;
    }
    Ok(resolved)
}

/// Write the two staged legs as a real TRANSFER_OUT/TRANSFER_IN pair sharing
/// a fresh transfer_id, and mark both staging rows resolved. Callable both
/// from auto-matching (exact asset+qty match found automatically) and later
/// from a manual "connect these two rows" action.
pub fn resolve_pending_transfer_pair(
    conn: &Connection,
    out_staging_id: i64,
    in_staging_id: i64,
) -> Result<(), String> {
    let out_row = load_pending_leg(conn, out_staging_id, "OUT")?;
    let in_row = load_pending_leg(conn, in_staging_id, "IN")?;
    if out_row.asset != in_row.asset || out_row.qty != in_row.qty {
        return Err(format!(
            "Transfer legs don't match: {} {} out vs {} {} in",
            out_row.qty, out_row.asset, in_row.qty, in_row.asset
        ));
    }

    let transfer_id = uuid::Uuid::new_v4().to_string();
    let created_at = Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO ledger_entries
            (holding_id, entry_type, asset, qty, transfer_id, event_at, entry_source, import_batch_id, created_at)
         VALUES (?1, 'TRANSFER_OUT', ?2, ?3, ?4, ?5, 'CSV_IMPORT', ?6, ?7)",
        rusqlite::params![out_row.holding_id, out_row.asset, out_row.qty.to_string(), transfer_id, out_row.event_at, out_row.import_batch_id, created_at],
    )
    .map_err(|e| e.to_string())?;
    let out_ledger_id = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO ledger_entries
            (holding_id, entry_type, asset, qty, transfer_id, event_at, entry_source, import_batch_id, created_at)
         VALUES (?1, 'TRANSFER_IN', ?2, ?3, ?4, ?5, 'CSV_IMPORT', ?6, ?7)",
        rusqlite::params![in_row.holding_id, in_row.asset, in_row.qty.to_string(), transfer_id, in_row.event_at, in_row.import_batch_id, created_at],
    )
    .map_err(|e| e.to_string())?;
    let in_ledger_id = conn.last_insert_rowid();

    conn.execute(
        "UPDATE import_staging_rows
         SET decision = 'RESOLVE_TRANSFER', resolved_transfer_id = ?1,
             resolved_ledger_entry_id = ?2, paired_staging_row_id = ?3
         WHERE id = ?4",
        rusqlite::params![transfer_id, out_ledger_id, in_staging_id, out_staging_id],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE import_staging_rows
         SET decision = 'RESOLVE_TRANSFER', resolved_transfer_id = ?1,
             resolved_ledger_entry_id = ?2, paired_staging_row_id = ?3
         WHERE id = ?4",
        rusqlite::params![transfer_id, in_ledger_id, out_staging_id, in_staging_id],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

struct PendingLeg {
    holding_id: i64,
    asset: String,
    qty: Decimal,
    event_at: String,
    import_batch_id: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::csv_parse::{CsvFormat, parse_csv};
    use rust_decimal_macros::dec;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        crate::migrations::run(&conn).unwrap();
        // Two holdings under one entity, at two different venues, so a
        // transfer between them is realistic.
        conn.execute("INSERT INTO entities (name, entity_type) VALUES ('Personal', 'PERSONAL')", [])
            .unwrap();
        conn.execute("INSERT INTO venues (name, venue_type) VALUES ('Coinbase', 'EXCHANGE')", [])
            .unwrap();
        conn.execute("INSERT INTO venues (name, venue_type) VALUES ('DWP', 'CUSTODIAN')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO holdings (venue_id, entity_id, name, is_default_holding) VALUES (1, 1, 'Default', 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO holdings (venue_id, entity_id, name, is_default_holding) VALUES (2, 1, 'Business', 0)",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn buy_row_stages_as_new_and_commits_to_ledger_entries() {
        let mut conn = test_conn();
        let (rows, errors) = parse_csv(
            CsvFormat::GenericPlaceholder,
            "Date,Type,Asset,Quantity,Price,Amount\n1/1/2026,BUY,XRP,100,2.00,200.00\n",
        );
        assert!(errors.is_empty());

        let batch_id = create_import_batch(&conn, 1, "test.csv", rows.len()).unwrap();
        stage_parsed_rows(&conn, batch_id, 1, &rows).unwrap();

        let staged = list_staging_rows(&conn, batch_id).unwrap();
        assert_eq!(staged.len(), 1);
        assert_eq!(staged[0].classification, "NEW");

        resolve_staging_row(&conn, staged[0].id, "ACCEPT").unwrap();
        let summary = commit_batch(&mut conn, batch_id).unwrap();
        assert_eq!(summary.ledger_entries_committed, 1);

        let qty: String = conn
            .query_row("SELECT qty FROM ledger_entries WHERE entry_type = 'BUY'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(Decimal::from_str(&qty).unwrap(), dec!(100));
    }

    #[test]
    fn re_staging_the_same_row_is_matched_existing_not_new() {
        let mut conn = test_conn();
        let (rows, _) = parse_csv(
            CsvFormat::GenericPlaceholder,
            "Date,Type,Asset,Quantity,Price,Amount\n1/1/2026,BUY,XRP,100,2.00,200.00\n",
        );

        let batch1 = create_import_batch(&conn, 1, "first.csv", rows.len()).unwrap();
        stage_parsed_rows(&conn, batch1, 1, &rows).unwrap();
        let staged1 = list_staging_rows(&conn, batch1).unwrap();
        resolve_staging_row(&conn, staged1[0].id, "ACCEPT").unwrap();
        commit_batch(&mut conn, batch1).unwrap();

        // Re-import the identical row (e.g. the same file imported twice).
        let batch2 = create_import_batch(&conn, 1, "first.csv", rows.len()).unwrap();
        stage_parsed_rows(&conn, batch2, 1, &rows).unwrap();
        let staged2 = list_staging_rows(&conn, batch2).unwrap();
        assert_eq!(staged2[0].classification, "MATCHED_EXISTING");
    }

    #[test]
    fn advisory_fee_commits_to_cash_activity_not_ledger() {
        let mut conn = test_conn();
        let (rows, _) = parse_csv(
            CsvFormat::DwpAnchorage,
            "Transacted Date,Settled Date,Client Name,Account,Rep Code,Firm,Symbol,Description,Type,Quantity,Price,Amount,Fee,Custodian,Access,Action,Source,Account Type,Tax Tag,Tax Date\n\
             1/9/2026,1/10/2026,\"Sample, Client\",9999,CM0000,DWP,USD,ADVISORY_FEE -50.00 USD at $1.00,ADVISORY FEE,-50.00,$1.00 ,($50.00),0,Anchorage,Tradable,,Anchorage,Limited Liability Company (LLC),,\n",
        );
        let batch_id = create_import_batch(&conn, 2, "fees.csv", rows.len()).unwrap();
        stage_parsed_rows(&conn, batch_id, 2, &rows).unwrap();
        let staged = list_staging_rows(&conn, batch_id).unwrap();
        assert_eq!(staged[0].target_table, "CASH_ACTIVITY_ENTRIES");

        resolve_staging_row(&conn, staged[0].id, "ACCEPT").unwrap();
        let summary = commit_batch(&mut conn, batch_id).unwrap();
        assert_eq!(summary.cash_activity_entries_committed, 1);
        assert_eq!(summary.ledger_entries_committed, 0);

        let count: i64 = conn.query_row("SELECT COUNT(*) FROM ledger_entries", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 0);
    }

    // The scenario from the real import: coins leave one holding
    // (WITHDRAWAL/SUBACCOUNT_TRANSFER) in one file and arrive at another
    // holding (DEPOSIT) in a completely different file. Neither commit can
    // see the other's data at parse time, so this checks that committing the
    // second batch auto-resolves against the first batch's still-pending leg.
    #[test]
    fn pending_transfer_legs_from_two_different_batches_auto_resolve_on_commit() {
        let mut conn = test_conn();

        let (out_rows, _) = parse_csv(
            CsvFormat::GenericPlaceholder,
            "Date,Type,Asset,Quantity,Price,Amount\n6/1/2025,WITHDRAWAL,XRP,1000,2.00,2000.00\n",
        );
        let batch_out = create_import_batch(&conn, 1, "coinbase.csv", out_rows.len()).unwrap();
        stage_parsed_rows(&conn, batch_out, 1, &out_rows).unwrap();
        // Committing with nothing ACCEPTed still attempts auto-resolution —
        // the pending leg alone has no match yet.
        let summary = commit_batch(&mut conn, batch_out).unwrap();
        assert_eq!(summary.transfers_auto_resolved, 0);
        assert!(!list_pending_transfers(&conn).unwrap().is_empty());

        let (in_rows, _) = parse_csv(
            CsvFormat::DwpAnchorage,
            "Transacted Date,Settled Date,Client Name,Account,Rep Code,Firm,Symbol,Description,Type,Quantity,Price,Amount,Fee,Custodian,Access,Action,Source,Account Type,Tax Tag,Tax Date\n\
             6/3/2025,6/3/2025,\"Sample, Client\",9999,CM0000,DWP,XRP,DEPOSIT 1000 XRP at $2.05,DEPOSIT,1000,$2.05 ,\"$2050.00\",0,Anchorage,Tradable,,Anchorage,Limited Liability Company (LLC),,\n",
        );
        let batch_in = create_import_batch(&conn, 2, "dwp.csv", in_rows.len()).unwrap();
        stage_parsed_rows(&conn, batch_in, 2, &in_rows).unwrap();

        let summary = commit_batch(&mut conn, batch_in).unwrap();
        assert_eq!(summary.transfers_auto_resolved, 1);
        assert!(list_pending_transfers(&conn).unwrap().is_empty());

        let transfer_out_holding: i64 = conn
            .query_row("SELECT holding_id FROM ledger_entries WHERE entry_type = 'TRANSFER_OUT'", [], |r| r.get(0))
            .unwrap();
        let transfer_in_holding: i64 = conn
            .query_row("SELECT holding_id FROM ledger_entries WHERE entry_type = 'TRANSFER_IN'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(transfer_out_holding, 1);
        assert_eq!(transfer_in_holding, 2);

        let (out_transfer_id, in_transfer_id): (String, String) = (
            conn.query_row("SELECT transfer_id FROM ledger_entries WHERE entry_type = 'TRANSFER_OUT'", [], |r| r.get(0)).unwrap(),
            conn.query_row("SELECT transfer_id FROM ledger_entries WHERE entry_type = 'TRANSFER_IN'", [], |r| r.get(0)).unwrap(),
        );
        assert_eq!(out_transfer_id, in_transfer_id);
    }
}

fn load_pending_leg(conn: &Connection, staging_id: i64, expected_leg: &str) -> Result<PendingLeg, String> {
    conn.query_row(
        "SELECT parsed_holding_id, parsed_asset, parsed_qty, parsed_timestamp, import_batch_id
         FROM import_staging_rows
         WHERE id = ?1 AND classification = 'PENDING_TRANSFER' AND transfer_leg = ?2 AND decision IS NULL",
        rusqlite::params![staging_id, expected_leg],
        |r| {
            let holding_id: i64 = r.get(0)?;
            let asset: String = r.get(1)?;
            let qty_raw: String = r.get(2)?;
            let event_at: String = r.get(3)?;
            let import_batch_id: i64 = r.get(4)?;
            Ok((holding_id, asset, qty_raw, event_at, import_batch_id))
        },
    )
    .map_err(|e| format!("staging row {staging_id} is not an unresolved {expected_leg} leg: {e}"))
    .and_then(|(holding_id, asset, qty_raw, event_at, import_batch_id)| {
        Ok(PendingLeg {
            holding_id,
            asset,
            qty: Decimal::from_str(&qty_raw).map_err(|e| e.to_string())?,
            event_at,
            import_batch_id,
        })
    })
}
