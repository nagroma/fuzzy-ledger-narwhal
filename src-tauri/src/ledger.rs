// Pure fold layer: walks the immutable ledger chronologically into current
// Lots + DisposalLines + review flags. No tauri, no rusqlite — same purity
// rule as lots.rs, so this stays fast to test and easy to reason about.
//
// One thing this module owns that lots.rs doesn't: TRANSFER_IN/TRANSFER_OUT
// pairing. A transfer between two of the end user's own holdings is
// non-taxable, but
// the basis and acquisition date of every lot fragment moved must travel
// with the coins — so a TRANSFER_OUT disposes FIFO (same as a SELL) purely to
// find out *which* fragments move, discards the throwaway proceeds figure,
// and re-creates each fragment as a new lot under the paired TRANSFER_IN's
// holding, with the original cost basis and acquired_at untouched.

use crate::lots::{dispose_fifo, DisposalLine, Lot, LotError};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryType {
    Buy,
    Sell,
    TradeIn,
    TradeOut,
    TransferIn,
    TransferOut,
    GiftOut,
    Income,
    FeeBurn,
    DustIn,
}

/// One row of the immutable ledger, in the engine's native types (Decimal/
/// DateTime, not wire strings) — the seam commands.rs converts SQL rows into
/// before calling `fold_ledger`.
#[derive(Debug, Clone)]
pub struct LedgerRow {
    pub id: i64,
    pub holding_id: i64,
    pub entry_type: EntryType,
    pub asset: String,
    pub qty: Decimal,
    pub cost_basis_per_unit: Option<Decimal>,
    pub proceeds_total: Option<Decimal>,
    pub transfer_id: Option<String>,
    pub event_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewReason {
    /// A TRANSFER pair whose two holdings resolve to different entities.
    CrossEntityTransfer,
    /// XRP-style network fee burn: in-kind destruction, tax treatment unsettled.
    FeeBurnUnsettled,
    /// De minimis unsolicited incoming — flagged for the CPA, never silently
    /// booked as ordinary income or dropped.
    DustIncome,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewFlag {
    pub entry_id: i64,
    pub reason: ReviewReason,
}

#[derive(Debug, PartialEq)]
pub enum LedgerError {
    Lot(LotError),
    /// A TRANSFER_OUT with no matching TRANSFER_IN sharing its transfer_id —
    /// a data-entry error (only one leg was entered), not something to guess
    /// a destination for.
    UnpairedTransfer { transfer_id: String },
}

impl From<LotError> for LedgerError {
    fn from(e: LotError) -> Self {
        LedgerError::Lot(e)
    }
}

#[derive(Debug, Default)]
pub struct FoldResult {
    pub lots: Vec<Lot>,
    pub disposals: Vec<DisposalLine>,
    pub flags: Vec<ReviewFlag>,
}

/// Fold the full ledger (already ordered by `event_at, id` — the same order
/// `dispose_fifo`'s FIFO relies on) into current lots, every disposal line
/// produced along the way, and review flags. `holding_entity` maps every
/// holding_id to its entity_id, needed only to detect cross-entity transfers.
pub fn fold_ledger(
    rows: &[LedgerRow],
    holding_entity: &HashMap<i64, i64>,
) -> Result<FoldResult, LedgerError> {
    // TRANSFER_IN rows indexed by transfer_id, so a TRANSFER_OUT can find its
    // destination holding regardless of which leg was entered first — both
    // legs of a transfer share the same instant in practice, and fold order
    // only needs to be correct with respect to *other* rows, not between the
    // two legs of one transfer.
    let transfer_ins: HashMap<&str, &LedgerRow> = rows
        .iter()
        .filter(|r| r.entry_type == EntryType::TransferIn)
        .filter_map(|r| r.transfer_id.as_deref().map(|tid| (tid, r)))
        .collect();

    let mut lots = Vec::new();
    let mut disposals = Vec::new();
    let mut flags = Vec::new();

    for row in rows {
        match row.entry_type {
            EntryType::Buy | EntryType::Income | EntryType::TradeIn => {
                lots.push(acquisition_lot(row));
            }
            EntryType::DustIn => {
                lots.push(acquisition_lot(row));
                flags.push(ReviewFlag {
                    entry_id: row.id,
                    reason: ReviewReason::DustIncome,
                });
            }
            EntryType::Sell | EntryType::TradeOut | EntryType::GiftOut | EntryType::FeeBurn => {
                let proceeds = row.proceeds_total.unwrap_or(Decimal::ZERO);
                let lines = dispose_fifo(
                    &mut lots,
                    row.holding_id,
                    &row.asset,
                    row.qty,
                    proceeds,
                    row.event_at,
                )?;
                disposals.extend(lines);
                if row.entry_type == EntryType::FeeBurn {
                    flags.push(ReviewFlag {
                        entry_id: row.id,
                        reason: ReviewReason::FeeBurnUnsettled,
                    });
                }
            }
            EntryType::TransferOut => {
                let transfer_id = row.transfer_id.as_deref().ok_or_else(|| {
                    LedgerError::UnpairedTransfer {
                        transfer_id: String::new(),
                    }
                })?;
                let dest = *transfer_ins.get(transfer_id).ok_or_else(|| {
                    LedgerError::UnpairedTransfer {
                        transfer_id: transfer_id.to_string(),
                    }
                })?;

                // Throwaway proceeds: only which fragments move matters here.
                // A transfer is non-taxable, so no gain/loss is ever computed
                // from this call — the DisposalLines are discarded, not
                // pushed into `disposals`.
                let lines = dispose_fifo(
                    &mut lots,
                    row.holding_id,
                    &row.asset,
                    row.qty,
                    Decimal::ZERO,
                    row.event_at,
                )?;
                for (i, line) in lines.iter().enumerate() {
                    lots.push(Lot {
                        id: synthetic_lot_id(dest.id, i),
                        holding_id: dest.holding_id,
                        asset: row.asset.clone(),
                        qty_remaining: line.qty,
                        // Recovered from the fragment's total basis; line.qty
                        // is never zero (dispose_fifo never emits a zero-qty
                        // fragment), so this division is exact.
                        cost_basis_per_unit: line.cost_basis / line.qty,
                        acquired_at: line.acquired_at,
                    });
                }

                let source_entity = holding_entity.get(&row.holding_id);
                let dest_entity = holding_entity.get(&dest.holding_id);
                if source_entity != dest_entity {
                    flags.push(ReviewFlag {
                        entry_id: row.id,
                        reason: ReviewReason::CrossEntityTransfer,
                    });
                }
            }
            EntryType::TransferIn => {
                // No-op: its lots were already synthesized when its paired
                // TRANSFER_OUT was folded, regardless of which leg appears
                // first in event order.
            }
        }
    }

    Ok(FoldResult {
        lots,
        disposals,
        flags,
    })
}

fn acquisition_lot(row: &LedgerRow) -> Lot {
    Lot {
        id: row.id,
        holding_id: row.holding_id,
        asset: row.asset.clone(),
        qty_remaining: row.qty,
        cost_basis_per_unit: row.cost_basis_per_unit.unwrap_or(Decimal::ZERO),
        acquired_at: row.event_at,
    }
}

/// A TRANSFER_OUT can split across several source lots (FIFO fragments), each
/// becoming its own destination lot. Real ledger entry ids are assigned by
/// SQLite AUTOINCREMENT and stay far under this multiplier at any realistic
/// volume (tens of thousands of rows, per the plan), so
/// `dest_entry_id * 1000 + fragment_index` can't collide with a real row id.
fn synthetic_lot_id(dest_entry_id: i64, fragment_index: usize) -> i64 {
    dest_entry_id * 1000 + fragment_index as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn ts(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn buy(id: i64, holding_id: i64, asset: &str, qty: Decimal, basis: Decimal, at: &str) -> LedgerRow {
        LedgerRow {
            id,
            holding_id,
            entry_type: EntryType::Buy,
            asset: asset.into(),
            qty,
            cost_basis_per_unit: Some(basis),
            proceeds_total: None,
            transfer_id: None,
            event_at: ts(at),
        }
    }

    // Trade 1 BTC (bought 2023-01-01 @ $20k) for 15 ETH worth $45k total on
    // the trade date. TRADE_OUT disposes the BTC lot: basis $20000, proceeds
    // $45000 (the FMV of what came back), gain $25000, long-term. TRADE_IN
    // creates a new ETH lot: cost_basis_per_unit = 45000/15 = $3000.
    #[test]
    fn trade_disposes_one_asset_and_acquires_another_at_fmv_basis() {
        let rows = vec![
            buy(1, 1, "BTC", dec!(1), dec!(20000), "2023-01-01T00:00:00Z"),
            LedgerRow {
                id: 2,
                holding_id: 1,
                entry_type: EntryType::TradeOut,
                asset: "BTC".into(),
                qty: dec!(1),
                cost_basis_per_unit: None,
                proceeds_total: Some(dec!(45000)),
                transfer_id: None,
                event_at: ts("2026-02-01T00:00:00Z"),
            },
            LedgerRow {
                id: 3,
                holding_id: 1,
                entry_type: EntryType::TradeIn,
                asset: "ETH".into(),
                qty: dec!(15),
                cost_basis_per_unit: Some(dec!(3000)),
                proceeds_total: None,
                transfer_id: None,
                event_at: ts("2026-02-01T00:00:00Z"),
            },
        ];
        let result = fold_ledger(&rows, &HashMap::new()).unwrap();

        assert_eq!(result.disposals.len(), 1);
        assert_eq!(result.disposals[0].cost_basis, dec!(20000));
        assert_eq!(result.disposals[0].proceeds, dec!(45000));
        assert_eq!(result.disposals[0].gain(), dec!(25000));
        assert!(result.disposals[0].long_term);

        let eth_lot = result.lots.iter().find(|l| l.asset == "ETH").unwrap();
        assert_eq!(eth_lot.qty_remaining, dec!(15));
        assert_eq!(eth_lot.cost_basis_per_unit, dec!(3000));
        assert!(result.flags.is_empty());
    }

    // Transfer 0.5 BTC from holding 1 to holding 2, both under entity 100
    // (the end user's own holdings). Source lot: 1 BTC @ $20k, acquired 2023-01-01.
    // Non-taxable: no disposal recorded. The 0.5 BTC arriving at holding 2
    // keeps its original basis ($20000/unit) and acquired_at (2023-01-01),
    // not the transfer date. Holding 1 keeps its other 0.5 BTC untouched.
    // Same-entity transfer → no review flag.
    #[test]
    fn transfer_pair_moves_basis_and_acquired_date_without_a_disposal() {
        let rows = vec![
            buy(1, 1, "BTC", dec!(1), dec!(20000), "2023-01-01T00:00:00Z"),
            LedgerRow {
                id: 2,
                holding_id: 1,
                entry_type: EntryType::TransferOut,
                asset: "BTC".into(),
                qty: dec!(0.5),
                cost_basis_per_unit: None,
                proceeds_total: None,
                transfer_id: Some("T1".into()),
                event_at: ts("2026-02-01T00:00:00Z"),
            },
            LedgerRow {
                id: 3,
                holding_id: 2,
                entry_type: EntryType::TransferIn,
                asset: "BTC".into(),
                qty: dec!(0.5),
                cost_basis_per_unit: None,
                proceeds_total: None,
                transfer_id: Some("T1".into()),
                event_at: ts("2026-02-01T00:00:00Z"),
            },
        ];
        let holding_entity = HashMap::from([(1, 100), (2, 100)]);
        let result = fold_ledger(&rows, &holding_entity).unwrap();

        assert!(result.disposals.is_empty());
        assert!(result.flags.is_empty());

        let source_remaining = result.lots.iter().find(|l| l.holding_id == 1).unwrap();
        assert_eq!(source_remaining.qty_remaining, dec!(0.5));

        let dest_lot = result.lots.iter().find(|l| l.holding_id == 2).unwrap();
        assert_eq!(dest_lot.qty_remaining, dec!(0.5));
        assert_eq!(dest_lot.cost_basis_per_unit, dec!(20000));
        assert_eq!(dest_lot.acquired_at, ts("2023-01-01T00:00:00Z"));
    }

    // Same transfer, but holding 1 belongs to entity 100 (Personal) and
    // holding 2 belongs to entity 200 (Business LLC) — must be flagged for
    // CPA review even though basis/date still move correctly.
    #[test]
    fn cross_entity_transfer_is_flagged_for_review() {
        let rows = vec![
            buy(1, 1, "BTC", dec!(1), dec!(20000), "2023-01-01T00:00:00Z"),
            LedgerRow {
                id: 2,
                holding_id: 1,
                entry_type: EntryType::TransferOut,
                asset: "BTC".into(),
                qty: dec!(0.5),
                cost_basis_per_unit: None,
                proceeds_total: None,
                transfer_id: Some("T1".into()),
                event_at: ts("2026-02-01T00:00:00Z"),
            },
            LedgerRow {
                id: 3,
                holding_id: 2,
                entry_type: EntryType::TransferIn,
                asset: "BTC".into(),
                qty: dec!(0.5),
                cost_basis_per_unit: None,
                proceeds_total: None,
                transfer_id: Some("T1".into()),
                event_at: ts("2026-02-01T00:00:00Z"),
            },
        ];
        let holding_entity = HashMap::from([(1, 100), (2, 200)]);
        let result = fold_ledger(&rows, &holding_entity).unwrap();

        assert_eq!(
            result.flags,
            vec![ReviewFlag {
                entry_id: 2,
                reason: ReviewReason::CrossEntityTransfer,
            }]
        );
    }

    // Gift 0.3 BTC out of a 1 BTC @ $15000 lot. Basis of the gifted fragment
    // = 0.3 * 15000 = $4500. GIFT_OUT proceeds are always $0 (no consideration
    // received) — the disposal line records the arithmetic, but whether it's
    // a recognized loss is a CPA/reporting question outside this layer.
    #[test]
    fn gift_out_disposes_a_lot_with_zero_proceeds() {
        let rows = vec![
            buy(1, 1, "BTC", dec!(1), dec!(15000), "2024-01-01T00:00:00Z"),
            LedgerRow {
                id: 2,
                holding_id: 1,
                entry_type: EntryType::GiftOut,
                asset: "BTC".into(),
                qty: dec!(0.3),
                cost_basis_per_unit: None,
                proceeds_total: Some(dec!(0)),
                transfer_id: None,
                event_at: ts("2026-02-01T00:00:00Z"),
            },
        ];
        let result = fold_ledger(&rows, &HashMap::new()).unwrap();

        assert_eq!(result.disposals.len(), 1);
        assert_eq!(result.disposals[0].cost_basis, dec!(4500));
        assert_eq!(result.disposals[0].proceeds, dec!(0));
        assert!(result.flags.is_empty());
    }

    // FEE_BURN (XRP network fee destruction) is always flagged — its tax
    // treatment is unsettled, so the app never silently decides it.
    #[test]
    fn fee_burn_is_always_flagged() {
        let rows = vec![
            buy(1, 1, "XRP", dec!(100), dec!(0.5), "2024-01-01T00:00:00Z"),
            LedgerRow {
                id: 2,
                holding_id: 1,
                entry_type: EntryType::FeeBurn,
                asset: "XRP".into(),
                qty: dec!(0.00001),
                cost_basis_per_unit: None,
                proceeds_total: Some(dec!(0)),
                transfer_id: None,
                event_at: ts("2026-02-01T00:00:00Z"),
            },
        ];
        let result = fold_ledger(&rows, &HashMap::new()).unwrap();

        assert_eq!(
            result.flags,
            vec![ReviewFlag {
                entry_id: 2,
                reason: ReviewReason::FeeBurnUnsettled,
            }]
        );
    }

    // DUST_IN (de minimis unsolicited incoming) is always flagged, and
    // defaults to $0 cost basis rather than guessing a FMV.
    #[test]
    fn dust_in_is_always_flagged() {
        let rows = vec![LedgerRow {
            id: 1,
            holding_id: 1,
            entry_type: EntryType::DustIn,
            asset: "XRP".into(),
            qty: dec!(0.001),
            cost_basis_per_unit: Some(dec!(0)),
            proceeds_total: None,
            transfer_id: None,
            event_at: ts("2026-02-01T00:00:00Z"),
        }];
        let result = fold_ledger(&rows, &HashMap::new()).unwrap();

        assert_eq!(result.lots.len(), 1);
        assert_eq!(result.lots[0].cost_basis_per_unit, dec!(0));
        assert_eq!(
            result.flags,
            vec![ReviewFlag {
                entry_id: 1,
                reason: ReviewReason::DustIncome,
            }]
        );
    }

    // A TRANSFER_OUT with no matching TRANSFER_IN (only one leg entered) is a
    // data-entry error, not something to guess a destination for.
    #[test]
    fn unpaired_transfer_is_an_error() {
        let rows = vec![
            buy(1, 1, "BTC", dec!(1), dec!(20000), "2023-01-01T00:00:00Z"),
            LedgerRow {
                id: 2,
                holding_id: 1,
                entry_type: EntryType::TransferOut,
                asset: "BTC".into(),
                qty: dec!(0.5),
                cost_basis_per_unit: None,
                proceeds_total: None,
                transfer_id: Some("T-missing".into()),
                event_at: ts("2026-02-01T00:00:00Z"),
            },
        ];
        let err = fold_ledger(&rows, &HashMap::new()).unwrap_err();

        assert_eq!(
            err,
            LedgerError::UnpairedTransfer {
                transfer_id: "T-missing".into()
            }
        );
    }
}
