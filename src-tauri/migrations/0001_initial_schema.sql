-- Replaces the prototype's two-table schema (wallets, BUY-only ledger_entries).
-- See architecture.md for the full rationale. Key points:
--
--   - entities (Personal / an LLC / an IRA / ...) are the tax-relevant owners.
--   - venues are where assets are held or accessed (a brokerage, a custodian,
--     a self-custody hardware wallet).
--   - holdings are the smallest unit with its own basis pool. Rev. Proc.
--     2024-28 per-wallet siloing keys off holding_id, not venue_id: one venue
--     (e.g. a custodian) can serve two different holdings under two different
--     entities, and their lots must never mix.
--   - addresses are per-(holding, asset) public receive/send addresses.
--   - ledger_entries is the immutable, append-only record. entry_type spans
--     acquisitions (BUY/INCOME/TRADE_IN/DUST_IN), disposals (SELL/TRADE_OUT/
--     GIFT_OUT/FEE_BURN), and non-taxable transfers (TRANSFER_IN/TRANSFER_OUT).
--   - import_batches/import_staging_rows support CSV import with duplicate
--     detection; dedup_hash is only ever populated for CSV-imported rows,
--     enforced by a CHECK, so manual entries can never be auto-matched.

CREATE TABLE entities (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    entity_type TEXT NOT NULL CHECK (entity_type IN ('PERSONAL','LLC','IRA','OTHER')),
    notes       TEXT
) STRICT;

CREATE TABLE venues (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE,
    venue_type TEXT NOT NULL CHECK (venue_type IN ('EXCHANGE','CUSTODIAN','HARDWARE_WALLET','OTHER')),
    notes      TEXT
) STRICT;

-- Smallest unit with its own basis pool. Two holdings at the same venue
-- (e.g. an LLC's sub-account and an IRA at the same custodian) must never
-- share lots -- see lots::dispose_fifo's holding_id filter.
CREATE TABLE holdings (
    id                  INTEGER PRIMARY KEY,
    venue_id            INTEGER NOT NULL REFERENCES venues(id),
    entity_id           INTEGER NOT NULL REFERENCES entities(id),
    name                TEXT NOT NULL,
    account_identifier  TEXT,
    is_default_holding  INTEGER NOT NULL DEFAULT 0 CHECK (is_default_holding IN (0,1)),
    UNIQUE (venue_id, name)
) STRICT;

-- Public receive/send address per (holding, asset). Storing the address
-- itself is not a privacy/security problem -- public addresses are
-- on-chain-visible by design. There is intentionally no column anywhere in
-- this schema for a private key or seed phrase; those must never be entered
-- into this app.
CREATE TABLE addresses (
    id         INTEGER PRIMARY KEY,
    holding_id INTEGER NOT NULL REFERENCES holdings(id),
    asset      TEXT NOT NULL,
    address    TEXT NOT NULL,
    label      TEXT,
    UNIQUE (holding_id, asset, address)
) STRICT;

CREATE TABLE import_batches (
    id              INTEGER PRIMARY KEY,
    venue_id        INTEGER NOT NULL REFERENCES venues(id),
    source_filename TEXT NOT NULL,
    imported_at     TEXT NOT NULL,
    row_count       INTEGER NOT NULL,
    accepted_count  INTEGER NOT NULL DEFAULT 0,
    notes           TEXT
) STRICT;

-- Immutable, append-only. Exactly one of (cost_basis_per_unit, proceeds_total)
-- is populated depending on whether the row is acquisition-shaped or
-- disposal-shaped; TRANSFER rows have neither -- basis/date are derived from
-- whichever lots get consumed (see ledger.rs), never typed in directly.
CREATE TABLE ledger_entries (
    id                   INTEGER PRIMARY KEY,
    holding_id           INTEGER NOT NULL REFERENCES holdings(id),
    entry_type           TEXT NOT NULL CHECK (entry_type IN (
                             'BUY','SELL','TRADE_IN','TRADE_OUT',
                             'TRANSFER_IN','TRANSFER_OUT',
                             'GIFT_OUT','INCOME','FEE_BURN','DUST_IN'
                         )),
    asset                TEXT NOT NULL,
    qty                  TEXT NOT NULL,
    cost_basis_per_unit  TEXT,
    proceeds_total       TEXT,
    trade_id             TEXT,
    transfer_id          TEXT,
    event_at             TEXT NOT NULL,
    entry_source         TEXT NOT NULL CHECK (entry_source IN ('MANUAL','CSV_IMPORT')),
    import_batch_id      INTEGER REFERENCES import_batches(id),
    dedup_hash           TEXT,
    note                 TEXT,
    created_at           TEXT NOT NULL,

    CHECK (
        (entry_type IN ('BUY','INCOME','TRADE_IN','DUST_IN')
            AND cost_basis_per_unit IS NOT NULL AND proceeds_total IS NULL)
        OR (entry_type IN ('SELL','TRADE_OUT','GIFT_OUT','FEE_BURN')
            AND proceeds_total IS NOT NULL AND cost_basis_per_unit IS NULL)
        OR (entry_type IN ('TRANSFER_IN','TRANSFER_OUT')
            AND cost_basis_per_unit IS NULL AND proceeds_total IS NULL)
    ),
    CHECK ((entry_type IN ('TRADE_IN','TRADE_OUT')) = (trade_id IS NOT NULL)),
    CHECK ((entry_type IN ('TRANSFER_IN','TRANSFER_OUT')) = (transfer_id IS NOT NULL)),
    CHECK ((entry_source = 'CSV_IMPORT') = (import_batch_id IS NOT NULL)),
    -- Manual rows can never be exact-hash-matched as duplicates: they never
    -- have a hash, by construction, not by matching-logic convention.
    CHECK (entry_source = 'CSV_IMPORT' OR dedup_hash IS NULL)
) STRICT;

CREATE UNIQUE INDEX idx_ledger_entries_dedup_hash
    ON ledger_entries (dedup_hash) WHERE dedup_hash IS NOT NULL;
CREATE INDEX idx_ledger_entries_holding_asset ON ledger_entries (holding_id, asset);

-- Non-crypto cash flows tied to a holding (advisory fees, lending interest,
-- etc.) -- reportable to the CPA, but never touch lots.rs/ledger.rs's Decimal
-- FIFO engine. There is no cost basis or gain/loss here, just a dollar
-- amount and a date.
CREATE TABLE cash_activity_entries (
    id              INTEGER PRIMARY KEY,
    holding_id      INTEGER NOT NULL REFERENCES holdings(id),
    kind            TEXT NOT NULL CHECK (kind IN ('ADVISORY_FEE','INTEREST_IN','INTEREST_OUT','OTHER')),
    currency        TEXT NOT NULL,
    amount          TEXT NOT NULL,
    event_at        TEXT NOT NULL,
    entry_source    TEXT NOT NULL CHECK (entry_source IN ('MANUAL','CSV_IMPORT')),
    import_batch_id INTEGER REFERENCES import_batches(id),
    note            TEXT,
    created_at      TEXT NOT NULL,

    CHECK ((entry_source = 'CSV_IMPORT') = (import_batch_id IS NOT NULL))
) STRICT;

-- Transient staging area for an in-progress CSV import review, persisted (not
-- just in-memory) so a multi-hundred-row reconciliation session survives an
-- app restart. `target_table` says where an ACCEPTed row gets written on
-- commit. A row classified PENDING_TRANSFER represents one leg of a
-- TRANSFER_IN/TRANSFER_OUT pair (e.g. a DEPOSIT/WITHDRAWAL/
-- SUBACCOUNT_TRANSFER-shaped CSV row) whose other leg may live in a
-- different import, a different venue's export, or may not exist yet at
-- all -- ledger.rs's invariant that every *committed* TRANSFER_OUT has a
-- real paired TRANSFER_IN is unchanged; a pending leg simply never reaches
-- ledger_entries until it's matched (by asset/qty/date) or manually
-- resolved. Until then it just sits here, flagged in the import UI, without
-- blocking anything else from being read.
CREATE TABLE import_staging_rows (
    id                         INTEGER PRIMARY KEY,
    import_batch_id            INTEGER NOT NULL REFERENCES import_batches(id),
    raw_row_json               TEXT NOT NULL,
    target_table               TEXT NOT NULL CHECK (target_table IN ('LEDGER_ENTRIES','CASH_ACTIVITY_ENTRIES')),
    parsed_holding_id          INTEGER REFERENCES holdings(id),
    parsed_entry_type          TEXT,
    parsed_asset               TEXT,
    parsed_qty                 TEXT,
    parsed_cost_basis_per_unit TEXT,
    parsed_proceeds_total      TEXT,
    parsed_timestamp           TEXT,
    exchange_txn_id            TEXT,
    computed_dedup_hash        TEXT,
    classification             TEXT NOT NULL CHECK (classification IN (
                                   'NEW','MATCHED_EXISTING','NEAR_DUPLICATE','PENDING_TRANSFER'
                               )),
    decision                   TEXT CHECK (decision IN (
                                   'ACCEPT','REJECT','TREAT_AS_NEW','TREAT_AS_DUPLICATE_SKIP','RESOLVE_TRANSFER'
                               )),
    resolved_ledger_entry_id   INTEGER REFERENCES ledger_entries(id),

    -- Only set when classification = 'PENDING_TRANSFER'.
    transfer_leg               TEXT CHECK (transfer_leg IN ('OUT','IN')),
    paired_staging_row_id      INTEGER REFERENCES import_staging_rows(id),
    resolved_transfer_id       TEXT,

    CHECK ((classification = 'PENDING_TRANSFER') = (transfer_leg IS NOT NULL))
) STRICT;
