# CryptoTracker

Windows desktop crypto transaction ledger. Tauri v2 + Rust + SQLite + React/TS.
End user is a non-programmer — needs a double-clickable installer, no runtime install.
Data lives in one SQLite file the user owns and can hand to a CPA.

## Non-negotiables

- **Per-holding lot siloing.** Rev. Proc. 2024-28: a disposal may only consume lots
  from the same holding (the smallest unit with its own basis pool — two holdings at
  the same venue, e.g. an LLC's sub-account and an IRA at the same custodian, are
  separate silos). Never relax the holding filter in `dispose_fifo`.
- **Never use floats for money or quantities.** `rust_decimal::Decimal` in Rust,
  TEXT in SQLite, strings across the IPC boundary. JS floats silently corrupt basis.
- **Multiply before dividing.** `a * b / c`, never `a * (b / c)` — Decimal can't
  represent repeating decimals, and division amplifies the error.
- **`lots.rs` and `ledger.rs` import nothing from tauri or rusqlite.** Keep the engine
  pure so tests stay fast and the logic stays portable. `ledger.rs` is the fold layer
  that walks the ledger into current lots/disposals/review flags; `commands.rs` is the
  only place that converts SQL rows into its input types and Decimal/DateTime into
  wire strings.
- **Never edit a passing test to make new code pass.** Ask me instead. The tests are
  hand-computed expected values — they're the spec, not the output.
- SQLite: `STRICT` tables, `PRAGMA foreign_keys = ON` per connection, no DECIMAL type.
- **Schema changes ship as a new migration file, never edits to one that already
  shipped.** `migrations::run` applies anything newer than the database's
  `PRAGMA user_version` on every `db::open()` — that's what brings an existing
  on-disk database (with real entries in it) up to date on a new release, not just
  what creates a fresh one. Exercise a new migration against a copy of a real
  (or realistically-sized) database before release, not just an empty one. If a
  future redesign is big enough to want a clean-slate rewrite, that rewrite is still
  a migration that transforms existing rows — never a step that silently drops them.
  **Temporary exception, in force now:** nothing has shipped to a real user yet, so
  until the first iteration of the schema is locked down, keep editing
  `0001_initial_schema.sql` directly instead of stacking new migration files — reset
  the dev DB and reseed rather than writing a migration for pre-release churn. This
  exception ends the moment there's a real on-disk database with real entries in it;
  from then on the rule above applies with no exceptions.
- **This repo is public (`nagroma/fuzzy-ledger-narwhal`, MIT-licensed) — never commit
  the end user's real name or his LLC's real name to it.** There was already one
  incident where both leaked into early planning docs and had to be scrubbed via a
  forced git-history rewrite; don't repeat it. Every doc, code comment, test, seed
  value, and sample fixture in this repo uses generic placeholders instead: entities
  are `Personal` / `Business LLC` / `IRA`, hardware wallet venues are
  `Tangem — Card A` / `Tangem — Card B`. Real names belong in exactly one place:
  `src-tauri/local_seed_overrides.json`, which is gitignored and never committed —
  `seed.rs::resolve_name` reads it (if present) to substitute real names into the
  *local* SQLite database only, for whoever's actually testing the app. See
  `local_seed_overrides.example.json` for the shape, and architecture.md's "Local
  name overrides" section. Third-party company/product names that aren't the end
  user's own identity (Coinbase, Digital Wealth Partners, Anchorage Digital, Tangem)
  are fine to use for real — the rule is about the end user's personal/company
  identity, not the venues he uses.
- **Keep `architecture.md` and `user-guide.md` in sync with the code.** Update both in
  the same change as any schema, entry-type, or import-semantics change — nothing
  currently enforces this automatically, so it's easy to let them drift.

## State

`cargo test` in `src-tauri/` is green (46 tests). Schema: `entities` / `venues` /
`holdings` / `addresses` / `import_batches` / `ledger_entries` / `cash_activity_entries`
/ `import_staging_rows` (full rationale in `architecture.md`). All entry types
(BUY/SELL/TRADE_IN/TRADE_OUT/TRANSFER_IN/TRANSFER_OUT/GIFT_OUT/INCOME/FEE_BURN/
DUST_IN) are wired end to end through `ledger.rs`'s fold layer, with per-type
commands and forms. Review flags (cross-entity transfers, FEE_BURN, DUST_IN) are
computed and surfaced in a "CPA review" tab.

CSV import is wired up (`src-tauri/src/import/`: `csv_parse.rs` pure parser,
`dedup.rs` pure SHA-256 hashing, `staging.rs` rusqlite CRUD + commit). One real
format is supported, `DWP_ANCHORAGE`; `GENERIC_PLACEHOLDER` exists only for venues
without a confirmed real export yet (currently Coinbase). Non-crypto cash flows
(advisory fees, etc.) land in `cash_activity_entries`, never in the FIFO engine. A
CSV row that's one leg of a transfer (DEPOSIT/WITHDRAWAL/SUBACCOUNT_TRANSFER-shaped)
is staged as `PENDING_TRANSFER` and never written to `ledger_entries` until its
other leg is found — automatically (exact asset+qty match, any batch, on commit) or
manually via the "Pending transfers" tab. `ledger.rs`'s invariant that every
*committed* TRANSFER_OUT has a real paired TRANSFER_IN is unchanged; unresolved legs
just never reach the ledger. Sample fixtures for manual testing live in
`src-tauri/sample_data/` (obfuscated, not real account data — see that folder's
README).

`seed.rs` seeds the real (partial) venue/holding inventory — using generic
placeholder names (see the privacy non-negotiable above) — since there's no CRUD UI
yet. Read-only view at `VenueHierarchy.tsx` / `list_venue_hierarchy`:
- Coinbase (exchange) — one Personal holding.
- Digital Wealth Partners (custodian) — holds assets directly, holdings "Business"
  and "IRA" so far; DWP's full account list will be supplied later and added here
  by hand.
- Anchorage Digital (custodian) — a *separate* venue: only a portion of the DWP
  assets were transferred out to Anchorage, where they can be loaned out for
  cash/crypto interest. The real split isn't pinned down yet — seeded as mirroring
  both DWP holdings ("Business", "IRA") as placeholder test data. **Open items:** (1)
  the real Anchorage holding structure once statements are available, (2) that
  lending activity (interest received/paid) doesn't map to an existing
  `cash_activity_entries.kind` beyond a generic bucket yet — resolve both before
  real Anchorage statements get imported.
- Tangem — Card A and Tangem — Card B (hardware wallets) — each physical card is
  its own venue with one default holding; both sit under the Personal entity.

## Other open items (see architecture.md's "What's not built yet" for full detail)

- No Venue/Holding/Address management UI — `seed.rs` is the only way to change the
  inventory.
- No disposals/Form-8949 view yet (`ledger.rs::FoldResult.disposals` is computed but
  unread).
- No way to reclassify a `PENDING_TRANSFER` staging row that turns out to have no
  matching leg.
- `README.md` needs a rewrite (was still describing the original BUY-only
  prototype) — in progress as of this handoff, verify it's current before trusting
  it.

## My background

Python dev, ex-C#/SQL Server. No Rust. Explain Rust-specific decisions briefly as
you go — I want to learn this, not just have it written for me.
