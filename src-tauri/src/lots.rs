// The lot engine. This is the part that must be right.
// Pure functions, no database, no UI, no Tauri. Just arithmetic you can test.

use chrono::{DateTime, Datelike, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// An acquisition. Created by a BUY, a TRANSFER_IN, or income (staking/airdrop).
/// `qty_remaining` shrinks as disposals consume it.
///
/// `holding_id` is an opaque partition key as far as this module is concerned —
/// it's the caller's job to feed it the smallest unit that has its own basis
/// pool. See `dispose_fifo`'s doc comment for why that's a *holding*, not a
/// venue: two holdings at the same venue (e.g. an LLC's sub-account and an
/// IRA at the same custodian) must never share lots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lot {
    pub id: i64,
    pub holding_id: i64,
    pub asset: String,
    pub qty_remaining: Decimal,
    pub cost_basis_per_unit: Decimal,
    pub acquired_at: DateTime<Utc>,
}

/// One line of Form 8949: a fragment of one lot, disposed.
/// A single sale usually produces several of these.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisposalLine {
    pub lot_id: i64,
    pub asset: String,
    pub qty: Decimal,
    pub acquired_at: DateTime<Utc>,
    pub disposed_at: DateTime<Utc>,
    pub cost_basis: Decimal,
    pub proceeds: Decimal,
    pub long_term: bool,
}

impl DisposalLine {
    pub fn gain(&self) -> Decimal {
        self.proceeds - self.cost_basis
    }
}

#[derive(Debug, PartialEq)]
pub enum LotError {
    /// Tried to dispose more than the holding holds. Usually means a missing import.
    InsufficientLots { asset: String, short_by: Decimal },
}

/// Held more than one year → long-term. Uses calendar anniversary, not 365 days.
fn is_long_term(acquired: DateTime<Utc>, disposed: DateTime<Utc>) -> bool {
    let anniversary = acquired
        .with_year(acquired.year() + 1)
        .unwrap_or(acquired);
    disposed > anniversary
}

/// Match a disposal against lots, FIFO, **within a single holding**.
///
/// The holding filter is not a convenience — it's Rev. Proc. 2024-28. Since
/// 2025-01-01 you may only consume lots held in the same holding as the
/// disposal. A holding is the smallest unit with its own basis pool: two
/// holdings at the same venue (e.g. an LLC's sub-account and an IRA at the
/// same custodian) are separate silos even though they share a venue.
/// Deleting that filter would silently produce wrong basis with no visible
/// symptom.
///
/// `lots` is mutated: consumed quantity is deducted.
pub fn dispose_fifo(
    lots: &mut Vec<Lot>,
    holding_id: i64,
    asset: &str,
    qty: Decimal,
    total_proceeds: Decimal,
    disposed_at: DateTime<Utc>,
) -> Result<Vec<DisposalLine>, LotError> {
    // Candidate lots: same holding, same asset, still holding something. Oldest first.
    let mut candidates: Vec<&mut Lot> = lots
        .iter_mut()
        .filter(|l| l.holding_id == holding_id && l.asset == asset && l.qty_remaining > Decimal::ZERO)
        .collect();
    candidates.sort_by_key(|l| l.acquired_at);

    let available: Decimal = candidates.iter().map(|l| l.qty_remaining).sum();
    if available < qty {
        return Err(LotError::InsufficientLots {
            asset: asset.to_string(),
            short_by: qty - available,
        });
    }

    let mut lines = Vec::new();
    let mut left = qty;
    let mut proceeds_allocated = Decimal::ZERO;

    for lot in candidates {
        if left.is_zero() {
            break;
        }
        let take = left.min(lot.qty_remaining);

        // Proceeds split pro-rata across the fragments of this sale, except the
        // final fragment: it takes whatever's left over so the fragments always
        // sum to total_proceeds exactly, even when total_proceeds * take / qty
        // doesn't divide evenly (e.g. total_proceeds / qty is a repeating decimal).
        let proceeds = if take == left {
            total_proceeds - proceeds_allocated
        } else {
            total_proceeds * take / qty
        };
        proceeds_allocated += proceeds;

        lines.push(DisposalLine {
            lot_id: lot.id,
            asset: asset.to_string(),
            qty: take,
            acquired_at: lot.acquired_at,
            disposed_at,
            cost_basis: lot.cost_basis_per_unit * take,
            proceeds,
            long_term: is_long_term(lot.acquired_at, disposed_at),
        });

        lot.qty_remaining -= take;
        left -= take;
    }

    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn ts(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn lot(id: i64, holding_id: i64, qty: Decimal, basis: Decimal, at: &str) -> Lot {
        Lot {
            id,
            holding_id,
            asset: "BTC".into(),
            qty_remaining: qty,
            cost_basis_per_unit: basis,
            acquired_at: ts(at),
        }
    }

    // Hand-computed: buy 1 BTC @ $20k, sell 1 BTC for $50k. Gain = $30k. Long-term.
    #[test]
    fn simple_fifo_single_lot() {
        let mut lots = vec![lot(1, 1, dec!(1), dec!(20000), "2023-01-15T00:00:00Z")];
        let out = dispose_fifo(&mut lots, 1, "BTC", dec!(1), dec!(50000), ts("2026-03-01T00:00:00Z"))
            .unwrap();

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].cost_basis, dec!(20000));
        assert_eq!(out[0].gain(), dec!(30000));
        assert!(out[0].long_term);
        assert_eq!(lots[0].qty_remaining, dec!(0));
    }

    // Hand-computed: lots of 0.5 @ $20k and 1.0 @ $30k. Sell 0.75 for $45k.
    //   Fragment A: 0.5 from lot 1. basis 0.5 * 20000 = 10000.
    //               proceeds 45000 * (0.5/0.75) = 30000. gain = 20000.
    //   Fragment B: 0.25 from lot 2. basis 0.25 * 30000 = 7500.
    //               proceeds 45000 * (0.25/0.75) = 15000. gain = 7500.
    //   Lot 2 retains 0.75.
    #[test]
    fn fifo_spans_two_lots_partial_consumption() {
        let mut lots = vec![
            lot(1, 1, dec!(0.5), dec!(20000), "2024-01-01T00:00:00Z"),
            lot(2, 1, dec!(1.0), dec!(30000), "2024-06-01T00:00:00Z"),
        ];
        let out = dispose_fifo(&mut lots, 1, "BTC", dec!(0.75), dec!(45000), ts("2026-03-01T00:00:00Z"))
            .unwrap();

        assert_eq!(out.len(), 2);
        assert_eq!(out[0].cost_basis, dec!(10000));
        assert_eq!(out[0].proceeds, dec!(30000));
        assert_eq!(out[1].cost_basis, dec!(7500));
        assert_eq!(out[1].proceeds, dec!(15000));
        assert_eq!(lots[0].qty_remaining, dec!(0));
        assert_eq!(lots[1].qty_remaining, dec!(0.75));
    }

    // THE IMPORTANT ONE. Rev. Proc. 2024-28.
    // Holding 1 (Coinbase) holds 1 BTC @ $20k, acquired earlier.
    // Holding 2 (Ledger)   holds 1 BTC @ $45k, acquired later.
    // Sell 1 BTC from holding 2 for $50k.
    // Universal FIFO (WRONG) would grab the $20k lot → gain $30k.
    // Per-holding FIFO (RIGHT) uses only holding 2 → basis $45k → gain $5k.
    #[test]
    fn per_holding_siloing_does_not_reach_across_holdings() {
        let mut lots = vec![
            lot(1, 1, dec!(1), dec!(20000), "2023-01-01T00:00:00Z"),
            lot(2, 2, dec!(1), dec!(45000), "2025-08-01T00:00:00Z"),
        ];
        let out = dispose_fifo(&mut lots, 2, "BTC", dec!(1), dec!(50000), ts("2026-03-01T00:00:00Z"))
            .unwrap();

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].lot_id, 2);
        assert_eq!(out[0].cost_basis, dec!(45000));
        assert_eq!(out[0].gain(), dec!(5000));
        assert!(!out[0].long_term); // held ~7 months
        assert_eq!(lots[0].qty_remaining, dec!(1)); // holding 1 untouched
    }

    // Same idea as the test above, but the scenario that actually motivated
    // renaming the partition key from wallet_id to holding_id: two holdings
    // at the *same venue* (e.g. an LLC's sub-account and an IRA, both at the
    // same custodian) must silo exactly as strictly as two different venues
    // do. The engine only ever sees an opaque holding_id, so this is really
    // exercising the same filter predicate — but it's worth locking in at
    // the unit level, since it's the scenario a "per-venue" (instead of
    // per-holding) mistake would fail on first.
    //
    // Holding 10 (custodian, LLC sub-account) holds 2 BTC @ $10k, older.
    // Holding 11 (same custodian, IRA sub-account) holds 1 BTC @ $60k, newer.
    // Sell 1 BTC from holding 11 for $70k. Must use holding 11's $60k lot
    // (gain $10k), never reach into holding 10's cheaper lot even though
    // both are "the same custodian."
    #[test]
    fn same_venue_different_holdings_still_silo_independently() {
        let mut lots = vec![
            lot(10, 10, dec!(2), dec!(10000), "2023-01-01T00:00:00Z"),
            lot(11, 11, dec!(1), dec!(60000), "2025-09-01T00:00:00Z"),
        ];
        let out = dispose_fifo(&mut lots, 11, "BTC", dec!(1), dec!(70000), ts("2026-03-01T00:00:00Z"))
            .unwrap();

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].lot_id, 11);
        assert_eq!(out[0].cost_basis, dec!(60000));
        assert_eq!(out[0].gain(), dec!(10000));
        assert_eq!(lots[0].qty_remaining, dec!(2)); // holding 10 untouched
    }

    // Selling more than the holding holds is an error, not a $0-basis guess.
    // In the real app this surfaces as "unclassified — CPA review".
    #[test]
    fn insufficient_lots_is_an_error() {
        let mut lots = vec![lot(1, 1, dec!(0.5), dec!(20000), "2024-01-01T00:00:00Z")];
        let err = dispose_fifo(&mut lots, 1, "BTC", dec!(1), dec!(50000), ts("2026-03-01T00:00:00Z"))
            .unwrap_err();

        assert_eq!(
            err,
            LotError::InsufficientLots { asset: "BTC".into(), short_by: dec!(0.5) }
        );
    }

    // Decimal, not float. 0.1 + 0.2 == 0.3 exactly. This is the whole reason
    // we don't use f64 anywhere near a basis calculation.
    #[test]
    fn decimal_arithmetic_is_exact() {
        assert_eq!(dec!(0.1) + dec!(0.2), dec!(0.3));
        assert_ne!(0.1_f64 + 0.2_f64, 0.3_f64); // the bug we're avoiding
    }

    // Pro-rata proceeds must sum exactly to total_proceeds. Splitting $100 seven
    // ways (7 lots of 1 BTC each, sold together) doesn't divide evenly, so
    // rounding each fragment independently drifts the sum off by 1e-26.
    // Hand-computed: 100/7 rounds to 14.285714285714285714285714286 per lot
    // (rust_decimal's ~28 sig-fig precision); 7 of those sum to
    // 100.00000000000000000000000001, one part in 10^26 over $100.00.
    #[test]
    fn pro_rata_proceeds_residual_ties_to_total() {
        let mut lots: Vec<Lot> = (1..=7)
            .map(|i| lot(i, 1, dec!(1), dec!(20000), "2024-01-01T00:00:00Z"))
            .collect();
        let out = dispose_fifo(&mut lots, 1, "BTC", dec!(7), dec!(100), ts("2026-01-01T00:00:00Z"))
            .unwrap();

        let sum: Decimal = out.iter().map(|l| l.proceeds).sum();
        assert_eq!(sum, dec!(100));

        // With the fix (residual on the last fragment), the first six fragments
        // stay pro-rata and the seventh absorbs whatever's left over exactly.
        for line in &out[..6] {
            assert_eq!(line.proceeds, dec!(14.285714285714285714285714286));
        }
        assert_eq!(out[6].proceeds, dec!(100) - dec!(14.285714285714285714285714286) * dec!(6));
    }
}
