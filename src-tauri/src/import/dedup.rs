// Pure hashing — no rusqlite. SHA-256 over normalized fields, so the same
// underlying transaction always hashes identically regardless of which CSV
// export it came from or how many times it's re-imported.

use rust_decimal::Decimal;
use sha2::{Digest, Sha256};

/// Normalized fields for one candidate ledger row. `holding_id` is part of
/// the hash because the same asset/qty/date could legitimately occur in two
/// different holdings without being duplicates of each other.
pub fn hash_ledger_row(
    holding_id: i64,
    entry_type: &str,
    asset: &str,
    qty: Decimal,
    event_at: &str,
) -> String {
    let normalized = format!("{holding_id}|{entry_type}|{asset}|{qty}|{event_at}");
    let digest = Sha256::digest(normalized.as_bytes());
    format!("{digest:x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn identical_inputs_hash_identically() {
        let a = hash_ledger_row(1, "BUY", "XRP", dec!(100), "2026-01-01");
        let b = hash_ledger_row(1, "BUY", "XRP", dec!(100), "2026-01-01");
        assert_eq!(a, b);
    }

    #[test]
    fn different_holdings_hash_differently() {
        let a = hash_ledger_row(1, "BUY", "XRP", dec!(100), "2026-01-01");
        let b = hash_ledger_row(2, "BUY", "XRP", dec!(100), "2026-01-01");
        assert_ne!(a, b);
    }
}
