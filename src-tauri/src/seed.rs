// Seed data for a fresh database: the entity taxonomy (Personal / Business LLC
// / IRA) plus the real venue/holding inventory known so far. There's no
// management UI yet (a later phase — see architecture.md), so this module
// *is* how the real Venue/Holding structure gets entered: insert-if-empty,
// same spirit as the prototype's seed_wallets, never overwrite what's
// already there. Extend this list by hand as more of the real inventory
// (e.g. DWP's full account list) is confirmed.
//
// Every name below is a generic placeholder ("Business LLC", "Tangem — Card
// A") — this repo is public, so the real LLC name and the real names on the
// two hardware wallet cards never appear in committed code. For local
// testing, where it's actually useful to see real names instead of
// placeholders, `resolve_name` swaps in real values from an optional,
// gitignored `local_seed_overrides.json` (see
// `local_seed_overrides.example.json` for the shape) — falling back to the
// placeholder whenever that file or a specific key is absent.

use rusqlite::Connection;
use std::collections::HashMap;
use std::error::Error;

pub fn seed_reference_data(conn: &Connection) -> Result<(), Box<dyn Error>> {
    seed_entities(conn)?;
    seed_venues(conn)?;
    Ok(())
}

/// Swaps a generic placeholder for a real name, if a local override exists.
/// Never used for anything committed to the repo — only for what actually
/// ends up in the (gitignored) local SQLite file.
fn resolve_name(placeholder: &str) -> String {
    load_local_overrides()
        .get(placeholder)
        .cloned()
        .unwrap_or_else(|| placeholder.to_string())
}

fn load_local_overrides() -> HashMap<String, String> {
    std::fs::read_to_string("local_seed_overrides.json")
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

fn seed_entities(conn: &Connection) -> Result<(), Box<dyn Error>> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM entities", [], |r| r.get(0))?;
    if count == 0 {
        conn.execute(
            "INSERT INTO entities (name, entity_type) VALUES (?1, 'PERSONAL')",
            [resolve_name("Personal")],
        )?;
        conn.execute(
            "INSERT INTO entities (name, entity_type) VALUES (?1, 'LLC')",
            [resolve_name("Business LLC")],
        )?;
        conn.execute(
            "INSERT INTO entities (name, entity_type) VALUES (?1, 'IRA')",
            [resolve_name("IRA")],
        )?;
    }
    Ok(())
}

/// Real venue/holding inventory, as confirmed so far (placeholder names —
/// see the module comment for how real names get substituted locally):
///
/// - **Coinbase** (exchange) — one Personal holding.
/// - **Digital Wealth Partners (DWP)** (custodian) — a crypto wealth-advisory
///   firm; clients' assets are actually custodied at Anchorage Digital (an
///   OCC-chartered crypto-native bank) in segregated multi-sig wallets, with
///   DWP acting as co-signer/advisor rather than holding assets itself. DWP
///   is still modeled as its own Venue here because it's the entity the end
///   user interacts with and the one that reports on these holdings — see
///   architecture.md. Two known holdings so far: "Business" and "IRA"; DWP's
///   full account list will be supplied later and added here by hand.
/// - **Anchorage Digital** (custodian) — a *separate* venue: DWP holds assets
///   directly (the "Business" and "IRA" holdings above), and only a portion
///   of those DWP assets were transferred out to Anchorage, where they can be
///   loaned out for cash or crypto interest. The real split isn't pinned down
///   yet (placeholder test data for now) — modeled as mirroring both DWP
///   holdings, "Business" and "IRA", pending the real structure. That lending
///   activity will eventually need its own ledger entries (interest
///   received, etc.) — not modeled yet, flagged as a follow-up once real
///   Anchorage statements are available.
/// - **Tangem — Card A** and **Tangem — Card B** (hardware wallets) — each
///   physical Tangem card is its own venue with one default holding. Both
///   cards are confirmed to sit under the Personal entity (neither is a
///   separate entity).
fn seed_venues(conn: &Connection) -> Result<(), Box<dyn Error>> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM venues", [], |r| r.get(0))?;
    if count > 0 {
        return Ok(());
    }

    let entity_id = |resolved_name: &str| -> Result<i64, Box<dyn Error>> {
        Ok(conn.query_row("SELECT id FROM entities WHERE name = ?1", [resolved_name], |r| r.get(0))?)
    };
    let personal = entity_id(&resolve_name("Personal"))?;
    let business = entity_id(&resolve_name("Business LLC"))?;
    let ira = entity_id(&resolve_name("IRA"))?;

    let insert_venue = |placeholder_name: &str, venue_type: &str| -> Result<i64, Box<dyn Error>> {
        conn.execute(
            "INSERT INTO venues (name, venue_type) VALUES (?1, ?2)",
            rusqlite::params![resolve_name(placeholder_name), venue_type],
        )?;
        Ok(conn.last_insert_rowid())
    };
    let insert_holding = |venue_id: i64, entity_id: i64, placeholder_name: &str, is_default: bool| -> Result<(), Box<dyn Error>> {
        conn.execute(
            "INSERT INTO holdings (venue_id, entity_id, name, is_default_holding) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![venue_id, entity_id, resolve_name(placeholder_name), is_default as i64],
        )?;
        Ok(())
    };

    let coinbase = insert_venue("Coinbase", "EXCHANGE")?;
    insert_holding(coinbase, personal, "Default", true)?;

    let dwp = insert_venue("Digital Wealth Partners", "CUSTODIAN")?;
    insert_holding(dwp, business, "Business", false)?;
    insert_holding(dwp, ira, "IRA", false)?;

    let anchorage = insert_venue("Anchorage Digital", "CUSTODIAN")?;
    insert_holding(anchorage, business, "Business", false)?;
    insert_holding(anchorage, ira, "IRA", false)?;

    let tangem_card_a = insert_venue("Tangem — Card A", "HARDWARE_WALLET")?;
    insert_holding(tangem_card_a, personal, "Default", true)?;

    let tangem_card_b = insert_venue("Tangem — Card B", "HARDWARE_WALLET")?;
    insert_holding(tangem_card_b, personal, "Default", true)?;

    Ok(())
}
