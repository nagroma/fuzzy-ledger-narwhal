# CryptoTracker — Architecture

Developer-facing reference for how the system is put together and why. Keep
this in sync with schema/entry-type/import-semantics changes — it drifts
otherwise. (`user-guide.md` is the end-user-facing counterpart; `CLAUDE.md`
is the standing instructions/non-negotiables for whoever — human or AI — is
implementing.)

## Stack

Tauri v2 + Rust + SQLite (via `rusqlite`, bundled) + React/TypeScript.

Chosen over .NET/Avalonia (wanted to avoid C#), Python/PySide6 (Windows
packaging trips Windows Defender/SmartScreen false positives more than a
Rust binary does), and Electron (150MB installs, no native decimal type).
The output is a real double-clickable Windows installer (`npm run tauri
build` → `.msi`/NSIS `.exe`) — no Rust/Node/Python required on the end
user's machine.

## Layering

```
lots.rs  ──────►  ledger.rs  ──────►  commands.rs  ──────►  models.rs (DTOs)
 (pure)            (pure)              (the only rusqlite/           │
                                         Decimal↔String                │
                                         boundary)                     ▼
                                                                  src/api.ts
                                                                        │
                                                                        ▼
                                                              React components
```

- **`lots.rs`** and **`ledger.rs`** import nothing from `tauri` or
  `rusqlite` — this is a CLAUDE.md non-negotiable, not a style preference.
  It's what keeps `cargo test` fast (no DB, no webview) and keeps the tax
  logic portable if the shell around it ever changes.
- **`commands.rs`** is the only file that converts SQL rows into
  `lots::Lot`/`ledger::LedgerRow` and converts `Decimal`/`DateTime` back into
  the `String` wire types every DTO uses. Every `#[tauri::command]` function
  lives here.
- **`models.rs`** holds the wire DTOs, each `#[derive(TS)]` + `#[ts(export)]`
  — `cargo test` regenerates the matching TypeScript in `src/bindings/*.ts`
  from these. If a DTO field changes and you forget to re-run `cargo test`,
  the frontend types silently go stale; there's no build-time check for
  this yet.
- **`src/api.ts`** is the one place in the frontend that calls
  `invoke(...)` — every command has a typed wrapper here, and every
  component imports from `api.ts`, never from `@tauri-apps/api/core`
  directly.

## Domain model: Entity / Venue / Holding / Address

```
Entity (tax-relevant owner)
   ▲
   │ (entity_id FK — attaches at Holding, not Venue)
   │
Venue (where assets are held/accessed: exchange, custodian, hardware wallet)
   └─ Holding (smallest unit with its own cost-basis pool)
         └─ Address (public address + asset symbol, per holding+asset)
```

- **Entity** — `entities` table: `Personal` / `Business LLC` / `IRA` (seeded
  in `seed.rs` — `Business LLC` is a generic placeholder for the end user's
  real LLC name; see "Local name overrides" below for how real names get
  substituted only in a local, gitignored dev database). The tax-relevant
  owner. A cross-entity transfer (money moving between, say, Personal and
  Business LLC) is legitimate but always gets flagged for CPA review — see
  Review flags below.
- **Venue** — `venues` table: a brokerage (Coinbase), a custodian (Digital
  Wealth Partners, Anchorage Digital), or a self-custody device (a hardware
  wallet card). **Each physical hardware wallet card is its own Venue row**
  — two different cards are two venues, not one shared venue.
- **Holding** — `holdings` table: the smallest unit with its own basis pool.
  **Entity attaches here, not at the Venue** — this is the whole reason the
  hierarchy has this shape: Digital Wealth Partners is one venue but holds
  two holdings under two different entities ("Business" under Business LLC,
  "IRA" under the IRA entity), and those two holdings' lots must never mix.
  A venue with only one implicit holding (a hardware wallet card, a plain
  Coinbase login) gets an auto-created `is_default_holding = 1` row, and the
  UI collapses that tier when there's nothing else to show
  (`HOLDING_DISPLAY_NAME_SQL` in `commands.rs`: shows just the venue's name
  for a default holding, "Venue — Holding" otherwise).
- **Address** — `addresses` table: one row per (holding, asset), storing
  both the public address string and the asset symbol. **Not** a
  privacy/security concern to store — public addresses are on-chain-visible
  by design. There is intentionally no column anywhere in this schema for a
  private key or seed phrase; those must never be entered into this app.
  Not yet populated by seed data or a management UI — `VenueHierarchy.tsx`
  can display them, but there's currently nothing to show.

### Real venue/holding inventory (seeded in `seed.rs`, insert-if-empty)

There's no CRUD UI yet, so `seed.rs` *is* how the real inventory gets
entered — extend it by hand as more is confirmed. Names below are the
generic placeholders committed to the repo.

| Venue | Type | Holdings | Notes |
|---|---|---|---|
| Coinbase | EXCHANGE | Personal (default) | |
| Digital Wealth Partners | CUSTODIAN | Business (Business LLC), IRA | Holds assets directly. Full account list to be supplied later. |
| Anchorage Digital | CUSTODIAN | Business (Business LLC), IRA | Separate venue from DWP — only a portion of DWP's assets were transferred out here, where they can be loaned for cash/crypto interest. **The real split isn't pinned down yet** — currently seeded as mirroring DWP's two holdings as placeholder data. |
| Tangem — Card A | HARDWARE_WALLET | Default (Personal) | Each physical card is its own venue. |
| Tangem — Card B | HARDWARE_WALLET | Default (Personal) | Confirmed to sit under the Personal entity, not a separate entity. |

DWP is a wealth-advisory firm; client assets are actually custodied at
Anchorage Digital (an OCC-chartered bank) in segregated multi-sig wallets
with DWP as co-signer — DWP is still modeled as its own Venue because it's
who the end user interacts with/reports through.

#### Local name overrides

`seed.rs::resolve_name` swaps a generic placeholder (`"Business LLC"`,
`"Tangem — Card A"`, etc.) for a real name at seed time, read from an
optional `src-tauri/local_seed_overrides.json` — gitignored, never
committed. Copy `local_seed_overrides.example.json` to
`local_seed_overrides.json` and fill in real values for local
testing/development, where seeing real names instead of placeholders is
actually useful. Delete `cryptotracker.sqlite` and relaunch to reseed after
changing it. Every doc, comment, test, and sample fixture in this repo uses
the generic placeholders — the real values only ever exist in that one
gitignored file and in the local SQLite database it seeds.

## The ledger: entry types and the fold layer

`ledger_entries` is immutable and append-only — nothing is ever updated or
deleted, only appended. `entry_type` spans three shapes:

| Shape | Types | Populated | Null |
|---|---|---|---|
| Acquisition | `BUY`, `INCOME`, `TRADE_IN`, `DUST_IN` | `cost_basis_per_unit` | `proceeds_total` |
| Disposal | `SELL`, `TRADE_OUT`, `GIFT_OUT`, `FEE_BURN` | `proceeds_total` | `cost_basis_per_unit` |
| Transfer | `TRANSFER_IN`, `TRANSFER_OUT` | neither | both |

(Enforced by a `CHECK` constraint in `0001_initial_schema.sql`, not just
convention.) `TRADE_IN`/`TRADE_OUT` share a `trade_id`; `TRANSFER_IN`/
`TRANSFER_OUT` share a `transfer_id`. Transfer rows have no typed-in basis —
see below for why.

Lots and disposals are **derived, not stored**: `ledger.rs::fold_ledger`
walks every `ledger_entries` row (ordered `event_at, id`) and produces:

- **`lots: Vec<Lot>`** — current holdings, keyed by an opaque `holding_id`
  the pure engine doesn't interpret (that's `lots::dispose_fifo`'s job).
- **`disposals: Vec<DisposalLine>`** — one line per Form-8949-shaped
  disposal fragment (not yet surfaced in any UI — the field exists,
  `#[warn(dead_code)]` on it is expected until a disposals/8949 view is
  built).
- **`flags: Vec<ReviewFlag>`** — see below.

Recomputing on every read is deliberate, not a placeholder: at the ledger's
realistic scale (~1.3K transactions over 2 years, tens of thousands at
most), refolding is trivial. Revisit only if profiling ever says otherwise.

### Per-holding lot siloing (Rev. Proc. 2024-28)

`lots::dispose_fifo` filters candidate lots to `holding_id == <the disposal's
holding>` before doing FIFO matching — **never** relax this filter. Since
2025-01-01, a disposal may only consume lots held in the same holding as the
disposal; two holdings at the same venue (DWP's "Business" and "IRA") are
separate silos even though they share a venue. `lots.rs`'s test suite locks
this in explicitly (`per_holding_siloing_does_not_reach_across_holdings`,
`same_venue_different_holdings_still_silo_independently`).

### Transfers: how basis and acquisition date travel with the coins

A `TRANSFER_OUT` calls `dispose_fifo` exactly like a `SELL` would — same
FIFO matching against the source holding's lots — but with a throwaway
proceeds figure (`Decimal::ZERO`). The resulting `DisposalLine` fragments are
**discarded**, not recorded as a taxable disposal; instead, `ledger.rs` uses
each fragment's original `cost_basis`/`acquired_at` to synthesize a new `Lot`
under the paired `TRANSFER_IN`'s holding. This is the whole mechanism by
which "the coins keep their original basis and holding period" is enforced
in code, not just described in a comment.

Synthetic lot IDs for transfer fragments are `dest_entry_id * 1000 +
fragment_index` (`ledger.rs::synthetic_lot_id`) — safe from collision with
real `ledger_entries` row IDs at any realistic transaction volume.

**Invariant:** every *committed* `TRANSFER_OUT` in `ledger_entries` must
have a matching `TRANSFER_IN` sharing its `transfer_id`, or `fold_ledger`
returns `LedgerError::UnpairedTransfer` and the *entire* fold fails (locked
in by the `unpaired_transfer_is_an_error` test). This is why CSV import
never writes a one-sided transfer leg directly into `ledger_entries` — see
Import below.

### Review flags

Computed by `fold_ledger`, not stored, surfaced via `list_review_flags` /
the "CPA review" tab:

- **`CrossEntityTransfer`** — a transfer pair whose two holdings resolve to
  different `entity_id`s (e.g. Personal → Business LLC). Legitimate, still
  flagged.
- **`FeeBurnUnsettled`** — every `FEE_BURN` row (XRP-style network fee
  destruction). Tax treatment is genuinely unsettled; the app never guesses.
- **`DustIncome`** — every `DUST_IN` row. Defaults to $0 cost basis by
  convention, always flagged rather than silently booked as income.

## Non-crypto cash activity

`cash_activity_entries` holds advisory fees and (eventually) Anchorage
lending interest — tied to a `holding_id`, reportable to the CPA, but
**never** touched by `lots.rs`/`ledger.rs`. There's no cost basis or
gain/loss here, just a dollar amount, a currency, and a date. `kind` is
`ADVISORY_FEE` / `INTEREST_IN` / `INTEREST_OUT` / `OTHER`.

**Open item:** Anchorage's lending activity (crypto loaned out, cash or
crypto interest paid/received) doesn't yet have a settled mapping to
`INTEREST_IN`/`INTEREST_OUT` — no real Anchorage statement has been parsed
yet to confirm the shape.

## CSV import (`src-tauri/src/import/`)

Same pure-core/thin-shell split as the rest of the engine:

- **`csv_parse.rs`** (pure, no rusqlite) — `parse_csv(format, content) ->
  (Vec<ParsedRow>, Vec<RowParseError>)`. `ParsedRow` is one of
  `LedgerCandidate` (BUY/SELL-shaped), `CashActivityCandidate` (fees),
  `PendingTransfer` (one leg of a transfer — see below), or `Ignored`
  (deliberately not written anywhere, e.g. the USD cash leg of an
  on-platform crypto↔USD conversion, since the crypto-symbol leg of the same
  trade already fully captures it).
  - `CsvFormat::DwpAnchorage` — the one confirmed real format, DWP/Anchorage's
    "onramp_transactions" export. Column mapping and Type handling
    (`BUY`/`SELL`/`ADVISORY FEE`/`DEPOSIT`/`WITHDRAWAL`/
    `SUBACCOUNT TRANSFER`) live in `parse_dwp_anchorage`. An unrecognized
    `Type` is a `RowParseError`, never a silent guess.
  - `CsvFormat::GenericPlaceholder` — **not a real confirmed format** — a
    minimal stand-in for venues without a real export sample yet (currently
    just Coinbase), used only to build test fixtures until a real export is
    obtained.
- **`dedup.rs`** (pure) — SHA-256 over normalized `(holding_id, entry_type,
  asset, qty, event_at)`. An exact hash match against an existing
  `ledger_entries.dedup_hash` classifies a staged row `MATCHED_EXISTING`.
  Manual entries can *never* have a `dedup_hash` (enforced by a `CHECK`, not
  matching-logic convention), so they can never be silently auto-matched as
  duplicates of a CSV row.
- **`staging.rs`** (the only rusqlite-touching file here) — stage → classify
  → resolve → commit:
  1. `stage_parsed_rows` classifies each `LedgerCandidate` as `NEW` /
     `MATCHED_EXISTING` (exact dedup-hash match) / `NEAR_DUPLICATE` (same
     holding+asset+entry_type+date, different quantity — always surfaced for
     a human decision, never auto-resolved).
  2. The user resolves each staged row: `ACCEPT` / `REJECT` / `TREAT_AS_NEW`
     / `TREAT_AS_DUPLICATE_SKIP`.
  3. `commit_batch` writes every `ACCEPT`/`TREAT_AS_NEW` row into its
     `target_table` (`LEDGER_ENTRIES` or `CASH_ACTIVITY_ENTRIES`), then
     calls `auto_resolve_pending_transfers`.

### Pending transfers — the cross-file/cross-venue reconciliation problem

A `DEPOSIT`/`WITHDRAWAL`/`SUBACCOUNT TRANSFER`-shaped CSV row represents one
leg of a transfer whose other leg may be in a *different* import batch, a
*different* venue's export, or may not exist yet at all (a hardware wallet
like a Tangem card produces no CSV export of its own). Since `ledger.rs`'s
invariant requires every committed `TRANSFER_OUT` to have a real paired
`TRANSFER_IN`, these rows are staged as `classification = 'PENDING_TRANSFER'`
and **never written to `ledger_entries` at all** until resolved:

- **Automatically**, on every `commit_batch` call: `auto_resolve_pending_transfers`
  looks for an exact asset+qty match between any unresolved `OUT` leg and
  any unresolved `IN` leg, across *every* batch (not just the one being
  committed) — this is what makes a Coinbase withdrawal in one import
  reconcile against a DWP deposit in a completely unrelated import.
- **Manually**, via `resolve_pending_transfer_pair` (surfaced in the
  "Pending transfers" tab, `PendingTransfers.tsx`) — pick an `OUT` leg and
  an `IN` leg and connect them by hand.

Either path writes a real `TRANSFER_OUT`/`TRANSFER_IN` pair sharing a fresh
`transfer_id` into `ledger_entries`, and marks both staging rows
`decision = 'RESOLVE_TRANSFER'`. Until resolved, a pending row sits in
`import_staging_rows` — visible in the UI, but invisible to `fold_ledger`
(which only ever reads `ledger_entries`), so an unresolved transfer never
blocks the rest of the app from working.

**Not yet built:** a way to reclassify a `PENDING_TRANSFER` row as something
else entirely (e.g. "this was actually a new acquisition, not a transfer in")
if it turns out never to have a matching leg.

### Sample fixtures

`src-tauri/sample_data/` — obfuscated test data (see that folder's
`README.md`): `dwp_anchorage_sample.csv` (the real DWP/Anchorage format,
every number perturbed, fake name/account) and
`coinbase_placeholder_sample.csv` (fabricated `GenericPlaceholder` format,
demonstrating the cross-venue pending-transfer scenario). Both are embedded
via `include_str!` in `csv_parse.rs`'s test module, so `cargo test` verifies
the *actual committed files* parse cleanly, not just hand-picked excerpts.

## Migrations

`src-tauri/src/migrations.rs` — a hand-rolled `PRAGMA user_version`-based
runner (no external crate). `MIGRATIONS` is an ordered `&[(name, sql)]`
list, each `.sql` file loaded via `include_str!` from
`src-tauri/migrations/`. `migrations::run` reads the database's current
`user_version`, applies anything newer inside a transaction, and bumps the
pragma — called on every `db::open()`, i.e. every app launch, so it upgrades
an *existing* on-disk database with real data, not just fresh installs.

**Standing rule (in force once the schema ships for real):** every schema
change is a new numbered file, never an edit to one that already shipped.
**Temporary exception, currently active:** nothing has shipped to a real
user yet, so `0001_initial_schema.sql` is still being edited directly as the
schema gets locked down — see `CLAUDE.md` for the exact terms and when this
exception ends.

## Frontend structure

- **`src/api.ts`** — every `invoke(...)` call, typed against
  `src/bindings/*.ts`.
- **`src/App.tsx`** — tab state for the two independent tab groups (which
  entry-type form is showing; which grid/tool is showing), refreshes
  entries/lots/review-flags together after any mutation.
- **`src/components/EntryForms/`** — one form component per entry-type
  family: `BuyForm`, `DisposalEntryForm.tsx` (shared by `SellForm`/
  `GiftOutForm`/`FeeBurnForm` — a `proceedsMode` prop toggles between a
  user-entered proceeds field and a fixed-$0 note), `TradeForm`,
  `TransferForm` (the only one needing two independent holding pickers),
  `IncomeForm`, `DustInForm`. `useHoldings.ts` is the shared "load holdings,
  default to the first one" hook.
- **`src/components/EntriesGrid.tsx` / `LotsGrid.tsx` / `ReviewFlagsGrid.tsx`**
  — read-only grids.
- **`src/components/VenueHierarchy.tsx`** — read-only Venue → Holding
  (collapsed when `is_default_holding`) → Address tree.
- **`src/components/ImportPreview.tsx`** — file picker (`tauri-plugin-dialog`)
  → format/holding selection → preview counts → per-row accept/reject grid
  → commit.
- **`src/components/PendingTransfers.tsx`** — lists every unresolved
  transfer leg (across all batches) in two columns (leaving / arriving),
  lets the user manually connect one of each.

No router — a flat tab-state single screen, consistent with the app's small
surface area so far.

## What's not built yet

- **Venue/Holding/Address management UI.** The hierarchy is entered by hand
  in `seed.rs`; there's no way to add/edit a venue, holding, or address from
  the app itself.
- **A disposals/Form-8949 view.** `ledger.rs::FoldResult.disposals` is
  computed but nothing reads it yet.
- **A second real CSV format.** Only DWP/Anchorage is confirmed; Coinbase
  and the Tangem cards (which likely never produce a CSV at all — see
  Pending transfers above) still need real handling.
- **Reclassifying a stuck `PENDING_TRANSFER` row.**
- **The real Anchorage holding split**, and a settled mapping for its
  lending interest (see Non-crypto cash activity above).
- **`user-guide.md`'s and this doc's own upkeep** — both need a line added
  to CLAUDE.md's workflow (not yet done) requiring them to be updated in the
  same change as any schema/entry-type/import-semantics change, since
  nothing currently enforces that.
- **`README.md` is stale** — still describes the original BUY-only,
  `list_wallets`-era prototype and references the two planning docs that
  were deleted once this file and `user-guide.md` existed. Needs a rewrite,
  not attempted in this pass.
