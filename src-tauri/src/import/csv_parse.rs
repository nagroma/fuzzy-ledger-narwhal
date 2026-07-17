// Pure CSV parsing: text in, ParsedRow out, no rusqlite/tauri. Same purity
// rule as lots.rs/ledger.rs — a parser bug should be catchable with a fast
// unit test against a literal CSV string, not by importing a real file.
//
// One venue's export format is confirmed real (DWP/Anchorage's
// "onramp_transactions" export). GenericPlaceholder exists only to exercise
// the multi-hop pending-transfer scenario in tests until a real Coinbase/
// hardware-wallet export format is available — it is NOT a confirmed real
// schema and should not be relied on for actual import.

use rust_decimal::Decimal;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CsvFormat {
    /// DWP/Anchorage "onramp_transactions" export. Columns (subset used):
    /// Transacted Date, Symbol, Type, Quantity, Price, Amount.
    DwpAnchorage,
    /// Not a real confirmed format — a stand-in for venues without a real
    /// export sample yet (e.g. Coinbase), used only to build multi-hop
    /// transfer test fixtures. Columns: Date,Type,Asset,Quantity,Price,Amount.
    GenericPlaceholder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferLeg {
    Out,
    In,
}

/// One CSV row, classified into what it means for our ledger — before any
/// holding is assigned or any database is touched.
#[derive(Debug, Clone, PartialEq)]
pub enum ParsedRow {
    /// A crypto acquisition or disposal: BUY/SELL/INCOME-shaped.
    LedgerCandidate {
        entry_type: &'static str, // "BUY" or "SELL" for now
        asset: String,
        qty: Decimal,
        cost_basis_per_unit: Option<Decimal>,
        proceeds_total: Option<Decimal>,
        event_at: String, // "YYYY-MM-DD"
    },
    /// A non-crypto cash flow (advisory fee, etc.) — never touches the FIFO
    /// engine.
    CashActivityCandidate {
        kind: &'static str, // "ADVISORY_FEE" | "OTHER"
        currency: String,
        amount: Decimal, // signed: negative for a fee paid out
        event_at: String,
    },
    /// One leg of a transfer whose other leg may be in a different import,
    /// a different venue's export, or may not exist yet.
    PendingTransfer {
        leg: TransferLeg,
        asset: String,
        qty: Decimal,
        event_at: String,
    },
    /// Deliberately not written anywhere — e.g. the USD cash leg of an
    /// on-platform crypto<->USD conversion, which the crypto-symbol leg of
    /// the same trade already fully captures.
    Ignored,
}

#[derive(Debug, PartialEq)]
pub struct RowParseError {
    pub row_number: usize,
    pub message: String,
}

pub fn parse_csv(format: CsvFormat, content: &str) -> (Vec<ParsedRow>, Vec<RowParseError>) {
    match format {
        CsvFormat::DwpAnchorage => parse_dwp_anchorage(content),
        CsvFormat::GenericPlaceholder => parse_generic_placeholder(content),
    }
}

/// Columns: Transacted Date,Settled Date,Client Name,Account,Rep Code,Firm,
/// Symbol,Description,Type,Quantity,Price,Amount,Fee,Custodian,Access,
/// Action,Source,Account Type,Tax Tag,Tax Date
fn parse_dwp_anchorage(content: &str) -> (Vec<ParsedRow>, Vec<RowParseError>) {
    let mut rows = Vec::new();
    let mut errors = Vec::new();

    let mut lines = content.lines();
    let Some(_header) = lines.next() else {
        return (rows, errors);
    };

    for (i, line) in lines.enumerate() {
        let row_number = i + 2; // +1 for 1-indexing, +1 for the header line
        if line.trim().is_empty() {
            continue;
        }
        let fields = split_csv_line(line);
        if fields.len() < 12 {
            errors.push(RowParseError {
                row_number,
                message: format!("expected at least 12 columns, got {}", fields.len()),
            });
            continue;
        }

        let transacted_date = fields[0].trim();
        let symbol = fields[6].trim().to_uppercase();
        let row_type = fields[8].trim().to_uppercase().replace('_', " ");
        let quantity_raw = fields[9].trim();
        let price_raw = strip_money(fields[10].trim());
        let amount_raw = strip_money(fields[11].trim());

        let event_at = match to_iso_date(transacted_date) {
            Ok(d) => d,
            Err(e) => {
                errors.push(RowParseError { row_number, message: e });
                continue;
            }
        };

        let quantity = match Decimal::from_str(quantity_raw) {
            Ok(q) => q,
            Err(_) => {
                errors.push(RowParseError {
                    row_number,
                    message: format!("bad quantity: {quantity_raw}"),
                });
                continue;
            }
        };

        match row_type.as_str() {
            "ADVISORY FEE" => {
                let amount = match Decimal::from_str(&amount_raw) {
                    Ok(a) => a,
                    Err(_) => {
                        errors.push(RowParseError {
                            row_number,
                            message: format!("bad amount: {amount_raw}"),
                        });
                        continue;
                    }
                };
                rows.push(ParsedRow::CashActivityCandidate {
                    kind: "ADVISORY_FEE",
                    currency: symbol,
                    amount,
                    event_at,
                });
            }
            "DEPOSIT" | "WITHDRAWAL" if symbol == "USD" => {
                // Not seen in the real sample; a cash-only movement, not a
                // crypto transfer leg.
                let amount = match Decimal::from_str(&amount_raw) {
                    Ok(a) => a,
                    Err(_) => {
                        errors.push(RowParseError {
                            row_number,
                            message: format!("bad amount: {amount_raw}"),
                        });
                        continue;
                    }
                };
                rows.push(ParsedRow::CashActivityCandidate {
                    kind: "OTHER",
                    currency: symbol,
                    amount,
                    event_at,
                });
            }
            "DEPOSIT" => rows.push(ParsedRow::PendingTransfer {
                leg: TransferLeg::In,
                asset: symbol,
                qty: quantity.abs(),
                event_at,
            }),
            "WITHDRAWAL" => rows.push(ParsedRow::PendingTransfer {
                leg: TransferLeg::Out,
                asset: symbol,
                qty: quantity.abs(),
                event_at,
            }),
            "SUBACCOUNT TRANSFER" => rows.push(ParsedRow::PendingTransfer {
                leg: if quantity.is_sign_negative() { TransferLeg::Out } else { TransferLeg::In },
                asset: symbol,
                qty: quantity.abs(),
                event_at,
            }),
            "BUY" | "SELL" if symbol == "USD" => {
                // The cash leg of an on-platform crypto<->USD conversion —
                // the paired crypto-symbol row already captures the trade.
                rows.push(ParsedRow::Ignored)
            }
            "BUY" => {
                let cost_basis_per_unit = match Decimal::from_str(&price_raw) {
                    Ok(p) => p,
                    Err(_) => {
                        errors.push(RowParseError {
                            row_number,
                            message: format!("bad price: {price_raw}"),
                        });
                        continue;
                    }
                };
                rows.push(ParsedRow::LedgerCandidate {
                    entry_type: "BUY",
                    asset: symbol,
                    qty: quantity.abs(),
                    cost_basis_per_unit: Some(cost_basis_per_unit),
                    proceeds_total: None,
                    event_at,
                });
            }
            "SELL" => {
                let proceeds_total = match Decimal::from_str(&amount_raw) {
                    Ok(a) => a.abs(),
                    Err(_) => {
                        errors.push(RowParseError {
                            row_number,
                            message: format!("bad amount: {amount_raw}"),
                        });
                        continue;
                    }
                };
                rows.push(ParsedRow::LedgerCandidate {
                    entry_type: "SELL",
                    asset: symbol,
                    qty: quantity.abs(),
                    cost_basis_per_unit: None,
                    proceeds_total: Some(proceeds_total),
                    event_at,
                });
            }
            other => errors.push(RowParseError {
                row_number,
                message: format!("unhandled Type: {other}"),
            }),
        }
    }

    (rows, errors)
}

/// Placeholder format only, for building multi-hop transfer test fixtures
/// until a real export from a non-custodian venue is available. Columns:
/// Date,Type,Asset,Quantity,Price,Amount
fn parse_generic_placeholder(content: &str) -> (Vec<ParsedRow>, Vec<RowParseError>) {
    let mut rows = Vec::new();
    let mut errors = Vec::new();

    let mut lines = content.lines();
    let Some(_header) = lines.next() else {
        return (rows, errors);
    };

    for (i, line) in lines.enumerate() {
        let row_number = i + 2;
        if line.trim().is_empty() {
            continue;
        }
        let fields = split_csv_line(line);
        if fields.len() < 6 {
            errors.push(RowParseError {
                row_number,
                message: format!("expected 6 columns, got {}", fields.len()),
            });
            continue;
        }

        let date = fields[0].trim();
        let row_type = fields[1].trim().to_uppercase();
        let asset = fields[2].trim().to_uppercase();
        let quantity_raw = fields[3].trim();
        let price_raw = fields[4].trim();
        let amount_raw = fields[5].trim();

        let event_at = match to_iso_date(date) {
            Ok(d) => d,
            Err(e) => {
                errors.push(RowParseError { row_number, message: e });
                continue;
            }
        };
        let quantity = match Decimal::from_str(quantity_raw) {
            Ok(q) => q,
            Err(_) => {
                errors.push(RowParseError {
                    row_number,
                    message: format!("bad quantity: {quantity_raw}"),
                });
                continue;
            }
        };

        match row_type.as_str() {
            "DEPOSIT" => rows.push(ParsedRow::PendingTransfer {
                leg: TransferLeg::In,
                asset,
                qty: quantity.abs(),
                event_at,
            }),
            "WITHDRAWAL" => rows.push(ParsedRow::PendingTransfer {
                leg: TransferLeg::Out,
                asset,
                qty: quantity.abs(),
                event_at,
            }),
            "BUY" => {
                let cost_basis_per_unit = Decimal::from_str(price_raw).ok();
                rows.push(ParsedRow::LedgerCandidate {
                    entry_type: "BUY",
                    asset,
                    qty: quantity.abs(),
                    cost_basis_per_unit,
                    proceeds_total: None,
                    event_at,
                });
            }
            "SELL" => {
                let proceeds_total = Decimal::from_str(amount_raw).ok().map(|a: Decimal| a.abs());
                rows.push(ParsedRow::LedgerCandidate {
                    entry_type: "SELL",
                    asset,
                    qty: quantity.abs(),
                    cost_basis_per_unit: None,
                    proceeds_total,
                    event_at,
                });
            }
            other => errors.push(RowParseError {
                row_number,
                message: format!("unhandled Type: {other}"),
            }),
        }
    }

    (rows, errors)
}

/// Minimal CSV field splitter: handles double-quoted fields containing
/// commas (e.g. `"Sample, Client"`), which is all these two formats need.
/// Not a general CSV parser (no escaped-quote support) — swap for the `csv`
/// crate if a future format needs more.
fn split_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for c in line.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                fields.push(current.clone());
                current.clear();
            }
            _ => current.push(c),
        }
    }
    fields.push(current);
    fields
}

/// Strips a leading currency symbol/BOM and thousands separators, e.g.
/// `"$1.43 "` -> `"1.43"`, `"($129,920.00)"` -> `"-129920.00"`.
fn strip_money(raw: &str) -> String {
    let negative = raw.starts_with('(') && raw.ends_with(')');
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    if negative && !cleaned.starts_with('-') {
        format!("-{cleaned}")
    } else {
        cleaned
    }
}

/// "M/D/YYYY" -> "YYYY-MM-DD".
fn to_iso_date(raw: &str) -> Result<String, String> {
    let raw = raw.trim_start_matches('\u{feff}'); // strip a UTF-8 BOM if present
    let parts: Vec<&str> = raw.split('/').collect();
    if parts.len() != 3 {
        return Err(format!("bad date: {raw}"));
    }
    let month: u32 = parts[0].parse().map_err(|_| format!("bad date: {raw}"))?;
    let day: u32 = parts[1].parse().map_err(|_| format!("bad date: {raw}"))?;
    let year: u32 = parts[2].parse().map_err(|_| format!("bad date: {raw}"))?;
    Ok(format!("{year:04}-{month:02}-{day:02}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    // The actual committed fixtures (sample_data/), not just inline test
    // strings — makes sure the real deliverable files parse cleanly, not
    // just a hand-picked excerpt.
    const DWP_ANCHORAGE_FIXTURE: &str =
        include_str!("../../sample_data/dwp_anchorage_sample.csv");
    const COINBASE_PLACEHOLDER_FIXTURE: &str =
        include_str!("../../sample_data/coinbase_placeholder_sample.csv");

    #[test]
    fn dwp_anchorage_fixture_file_parses_with_no_errors() {
        let (rows, errors) = parse_csv(CsvFormat::DwpAnchorage, DWP_ANCHORAGE_FIXTURE);
        assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
        assert_eq!(rows.len(), 33);

        let pending = rows.iter().filter(|r| matches!(r, ParsedRow::PendingTransfer { .. })).count();
        let cash = rows.iter().filter(|r| matches!(r, ParsedRow::CashActivityCandidate { .. })).count();
        let ledger = rows.iter().filter(|r| matches!(r, ParsedRow::LedgerCandidate { .. })).count();
        let ignored = rows.iter().filter(|r| matches!(r, ParsedRow::Ignored)).count();

        // 1 SUBACCOUNT TRANSFER + 1 WITHDRAWAL + 6 DEPOSIT rows = 8 pending
        // transfer legs; 7 ADVISORY FEE rows = 7 cash-activity candidates;
        // the rest split between real crypto-symbol trades and their
        // ignored USD cash legs.
        assert_eq!(pending, 8);
        assert_eq!(cash, 7);
        assert_eq!(ledger + ignored, 33 - pending - cash);
    }

    #[test]
    fn coinbase_placeholder_fixture_file_parses_with_no_errors() {
        let (rows, errors) = parse_csv(CsvFormat::GenericPlaceholder, COINBASE_PLACEHOLDER_FIXTURE);
        assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
        assert_eq!(rows.len(), 2);
        assert!(matches!(rows[0], ParsedRow::LedgerCandidate { entry_type: "BUY", .. }));
        assert!(matches!(
            rows[1],
            ParsedRow::PendingTransfer { leg: TransferLeg::Out, .. }
        ));
    }

    // Hand-picked rows from the real DWP/Anchorage sample (obfuscated
    // account/name), covering every Type this format is known to produce.
    const SAMPLE: &str = "Transacted Date,Settled Date,Client Name,Account,Rep Code,Firm,Symbol,Description,Type,Quantity,Price,Amount,Fee,Custodian,Access,Action,Source,Account Type,Tax Tag,Tax Date
2/5/2026,2/5/2026,\"Sample, Client\",9999,CM0000,Digital Wealth Partners LLC,USD,SELL -129920.00 USD at $1.00,SELL,-129920,$1.00 ,\"($129,920.00)\",0,Anchorage,Tradable,,Anchorage,Limited Liability Company (LLC),,
2/5/2026,2/5/2026,\"Sample, Client\",9999,CM0000,Digital Wealth Partners LLC,XRP,BUY 90732.979975 XRP at $1.43,BUY,90732.97998,$1.43 ,\"$129,920.00 \",0,Anchorage,Tradable,,Anchorage,Limited Liability Company (LLC),,
1/9/2026,1/10/2026,\"Sample, Client\",9999,CM0000,Digital Wealth Partners LLC,USD,ADVISORY_FEE -821.37 USD at $1.00,ADVISORY FEE,-821.37,$1.00 ,($821.37),0,Anchorage,Tradable,,Anchorage,Limited Liability Company (LLC),,
11/4/2025,11/4/2025,\"Sample, Client\",9999,CM0000,Digital Wealth Partners LLC,XRP,SUBACCOUNT_TRANSFER -60000 XRP at $2.13,SUBACCOUNT TRANSFER,-60000,$2.13 ,\"($127,787.80)\",0,Anchorage,Tradable,,Anchorage,Limited Liability Company (LLC),,
10/27/2025,10/27/2025,\"Sample, Client\",9999,CM0000,Digital Wealth Partners LLC,XRP,DEPOSIT 80000 XRP at $2.69,DEPOSIT,80000,$2.69 ,\"$215,489.60 \",0,Anchorage,Tradable,,Anchorage,Limited Liability Company (LLC),,
10/17/2025,10/20/2025,\"Sample, Client\",9999,CM0000,Digital Wealth Partners LLC,XRP,SELL -256.138 XRP at $2.30,SELL,-256.138,$2.30 ,($588.21),0,Anchorage,Tradable,,Anchorage,Limited Liability Company (LLC),,
";

    #[test]
    fn dwp_anchorage_usd_leg_of_a_trade_is_ignored() {
        let (rows, errors) = parse_csv(CsvFormat::DwpAnchorage, SAMPLE);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert_eq!(rows[0], ParsedRow::Ignored);
    }

    #[test]
    fn dwp_anchorage_buy_row_is_a_ledger_candidate() {
        let (rows, _) = parse_csv(CsvFormat::DwpAnchorage, SAMPLE);
        assert_eq!(
            rows[1],
            ParsedRow::LedgerCandidate {
                entry_type: "BUY",
                asset: "XRP".into(),
                qty: dec!(90732.97998),
                cost_basis_per_unit: Some(dec!(1.43)),
                proceeds_total: None,
                event_at: "2026-02-05".into(),
            }
        );
    }

    #[test]
    fn dwp_anchorage_advisory_fee_is_cash_activity_not_ledger() {
        let (rows, _) = parse_csv(CsvFormat::DwpAnchorage, SAMPLE);
        assert_eq!(
            rows[2],
            ParsedRow::CashActivityCandidate {
                kind: "ADVISORY_FEE",
                currency: "USD".into(),
                amount: dec!(-821.37),
                event_at: "2026-01-09".into(),
            }
        );
    }

    #[test]
    fn dwp_anchorage_subaccount_transfer_is_a_pending_out_leg() {
        let (rows, _) = parse_csv(CsvFormat::DwpAnchorage, SAMPLE);
        assert_eq!(
            rows[3],
            ParsedRow::PendingTransfer {
                leg: TransferLeg::Out,
                asset: "XRP".into(),
                qty: dec!(60000),
                event_at: "2025-11-04".into(),
            }
        );
    }

    #[test]
    fn dwp_anchorage_deposit_is_a_pending_in_leg() {
        let (rows, _) = parse_csv(CsvFormat::DwpAnchorage, SAMPLE);
        assert_eq!(
            rows[4],
            ParsedRow::PendingTransfer {
                leg: TransferLeg::In,
                asset: "XRP".into(),
                qty: dec!(80000),
                event_at: "2025-10-27".into(),
            }
        );
    }

    #[test]
    fn dwp_anchorage_sell_row_uses_absolute_proceeds() {
        let (rows, _) = parse_csv(CsvFormat::DwpAnchorage, SAMPLE);
        assert_eq!(
            rows[5],
            ParsedRow::LedgerCandidate {
                entry_type: "SELL",
                asset: "XRP".into(),
                qty: dec!(256.138),
                cost_basis_per_unit: None,
                proceeds_total: Some(dec!(588.21)),
                event_at: "2025-10-17".into(),
            }
        );
    }

    #[test]
    fn unrecognized_type_is_a_row_error_not_a_silent_guess() {
        let content = "Transacted Date,Settled Date,Client Name,Account,Rep Code,Firm,Symbol,Description,Type,Quantity,Price,Amount,Fee,Custodian,Access,Action,Source,Account Type,Tax Tag,Tax Date
1/1/2026,1/1/2026,\"Sample, Client\",9999,CM0000,Digital Wealth Partners LLC,XRP,MYSTERY -1 XRP,MYSTERY_TYPE,-1,$1.00 ,($1.00),0,Anchorage,Tradable,,Anchorage,Limited Liability Company (LLC),,
";
        let (rows, errors) = parse_csv(CsvFormat::DwpAnchorage, content);
        assert!(rows.is_empty());
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("MYSTERY TYPE"));
    }

    #[test]
    fn generic_placeholder_parses_deposit_and_withdrawal() {
        let content = "Date,Type,Asset,Quantity,Price,Amount
6/1/2025,WITHDRAWAL,XRP,1000,2.00,2000.00
";
        let (rows, errors) = parse_csv(CsvFormat::GenericPlaceholder, content);
        assert!(errors.is_empty());
        assert_eq!(
            rows[0],
            ParsedRow::PendingTransfer {
                leg: TransferLeg::Out,
                asset: "XRP".into(),
                qty: dec!(1000),
                event_at: "2025-06-01".into(),
            }
        );
    }
}
