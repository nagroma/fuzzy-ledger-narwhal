// Wire DTOs for the IPC boundary and the TS bindings generated from them.
//
// Every quantity/price/date crosses as a String, never a number — Decimal and
// DateTime don't round-trip through JSON safely, and a JS float anywhere near
// a cost basis is the exact bug this project exists to avoid. Parsing back to
// Decimal/DateTime happens only inside commands.rs, right at the boundary.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A holding, as offered to a picker/dropdown. `display_name` is a computed
/// convenience label (the venue's name alone for a single-holding venue,
/// "Venue — Holding" otherwise) — not the raw `holdings.name` column.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Holding {
    pub id: i64,
    pub display_name: String,
}

/// One address (public string + asset symbol) under a holding. Both are
/// shown to the user — the asset symbol is the primary label, the address
/// is displayed truncated with the full string on hover/copy. Never a
/// private key or seed phrase; there is no column anywhere for those.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AddressDto {
    pub id: i64,
    pub asset: String,
    pub address: String,
    pub label: Option<String>,
}

/// A holding, with its addresses, as shown in the read-only venue hierarchy
/// view.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HoldingHierarchyDto {
    pub holding_id: i64,
    pub holding_name: String,
    pub entity_name: String,
    pub account_identifier: Option<String>,
    pub is_default_holding: bool,
    pub addresses: Vec<AddressDto>,
}

/// One venue with all of its holdings, for the read-only Venue > Holding >
/// Address tree (`VenueHierarchy.tsx`). No CRUD UI yet — the real inventory
/// is entered by hand in `seed.rs` until a management screen exists.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct VenueHierarchyDto {
    pub venue_id: i64,
    pub venue_name: String,
    pub venue_type: String,
    pub holdings: Vec<HoldingHierarchyDto>,
}

/// What the BUY entry form submits: an acquisition of `qty` units of `asset`
/// in `holding_id`, at `cost_basis_per_unit` each.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NewLedgerEntry {
    pub holding_id: i64,
    pub asset: String,
    pub qty: String,
    pub cost_basis_per_unit: String,
    /// Date the asset was acquired, e.g. "2026-01-15".
    pub acquired_at: String,
}

/// A disposal: SELL for cash, or the taxable ones that aren't a SELL
/// (GIFT_OUT, FEE_BURN — proceeds are 0 for both, but they're still recorded
/// as disposals so basis leaves the ledger correctly).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NewDisposalEntry {
    pub holding_id: i64,
    pub asset: String,
    pub qty: String,
    pub proceeds_total: String,
    /// Date of the sale/gift/burn, e.g. "2026-01-15".
    pub event_at: String,
}

/// A crypto-to-crypto trade: one asset disposed, another acquired, both legs
/// at the same holding and the same moment, linked by a shared `trade_id`.
/// `fmv_total` is the fair-market-value of what came back — it becomes both
/// the TRADE_OUT's proceeds and the TRADE_IN's total cost basis.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NewTradeEntry {
    pub holding_id: i64,
    pub asset_out: String,
    pub qty_out: String,
    pub asset_in: String,
    pub qty_in: String,
    pub fmv_total: String,
    pub event_at: String,
}

/// A transfer between two of the end user's own holdings — non-taxable. Basis and
/// acquisition date travel with the coins; both legs share a `transfer_id`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NewTransferEntry {
    pub from_holding_id: i64,
    pub to_holding_id: i64,
    pub asset: String,
    pub qty: String,
    pub event_at: String,
}

/// Income (staking reward, airdrop, etc.): an acquisition at fair-market
/// value on the day received.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NewIncomeEntry {
    pub holding_id: i64,
    pub asset: String,
    pub qty: String,
    pub cost_basis_per_unit: String,
    pub event_at: String,
}

/// De minimis unsolicited incoming (dust). Always $0 cost basis by
/// convention — not user-entered — and always flagged for CPA review.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NewDustInEntry {
    pub holding_id: i64,
    pub asset: String,
    pub qty: String,
    pub event_at: String,
}

/// One row of the immutable ledger, as read back for the grid.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LedgerEntryDto {
    pub id: i64,
    pub holding_id: i64,
    pub holding_name: String,
    pub entry_type: String,
    pub asset: String,
    pub qty: String,
    pub cost_basis_per_unit: Option<String>,
    pub proceeds_total: Option<String>,
    pub trade_id: Option<String>,
    pub transfer_id: Option<String>,
    pub event_at: String,
    pub created_at: String,
}

/// Result of parsing + staging one CSV file: what a reconciliation review
/// screen needs before anything is written to the ledger.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImportPreviewDto {
    pub batch_id: i64,
    pub total_rows: i64,
    pub new_count: i64,
    pub matched_existing_count: i64,
    pub near_duplicate_count: i64,
    pub pending_transfer_count: i64,
    pub cash_activity_count: i64,
    pub parse_errors: Vec<String>,
}

/// One staged row, as shown in the import review grids.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StagingRowDto {
    pub id: i64,
    pub import_batch_id: i64,
    pub target_table: String,
    pub entry_type: Option<String>,
    pub asset: Option<String>,
    pub qty: Option<String>,
    pub cost_basis_per_unit: Option<String>,
    pub proceeds_total: Option<String>,
    pub event_at: Option<String>,
    pub classification: String,
    pub decision: Option<String>,
    pub transfer_leg: Option<String>,
}

/// Outcome of committing an import batch.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CommitSummaryDto {
    pub ledger_entries_committed: i64,
    pub cash_activity_entries_committed: i64,
    pub transfers_auto_resolved: i64,
}

/// A review flag surfaced alongside the ledger/lots view: something the app
/// deliberately didn't decide for the user (a cross-entity transfer, a
/// FEE_BURN whose tax treatment is unsettled, a DUST_IN parcel) and instead
/// hands to the CPA.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReviewFlagDto {
    pub entry_id: i64,
    pub reason: String,
}

/// A lot as derived from the ledger, for the "current holdings" grid. With
/// BUY-only entries this is a 1:1 projection of each entry (nothing has
/// consumed it yet) — but it's computed fresh from the ledger every time,
/// not stored, so the recompute-from-ledger principle holds even now.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LotDto {
    pub lot_id: i64,
    pub holding_id: i64,
    pub holding_name: String,
    pub asset: String,
    pub qty_remaining: String,
    pub cost_basis_per_unit: String,
    pub acquired_at: String,
}
