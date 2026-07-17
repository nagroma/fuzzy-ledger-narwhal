# Sample import files

Fixtures for manually exercising CSV import — never real account data.

- `dwp_anchorage_sample.csv` — the real DWP/Anchorage "onramp_transactions"
  export format, with every number obfuscated: fake client name/account/rep
  code, and every quantity and price independently perturbed by a random
  ±4–5% (seeded, reproducible, not reversible back to the real figures). Row
  order, dates, and the mix of Types (BUY/SELL/ADVISORY FEE/
  SUBACCOUNT TRANSFER/DEPOSIT) match the real file, since that structure is
  what the parser (`src/import/csv_parse.rs`) needs to be tested against.
  Import with format `DWP_ANCHORAGE`.

  Importing this file alone will leave its `DEPOSIT`/`SUBACCOUNT TRANSFER`
  rows as pending transfer legs (their other side isn't in this file) —
  that's expected, not a bug; see the "Pending transfers" tab.

- `coinbase_placeholder_sample.csv` — **not a real Coinbase export format**
  (no real sample was available when this was written) — a minimal
  fabricated stand-in, format `GENERIC_PLACEHOLDER`, used only to exercise
  the cross-venue pending-transfer scenario: a `WITHDRAWAL` row here (coins
  leaving Coinbase, headed for a Tangem card) has no matching `DEPOSIT` in
  this file or in `dwp_anchorage_sample.csv` — importing both files leaves
  two independent unresolved legs, demonstrating that an unresolved transfer
  is flagged in the UI without blocking anything else. Replace this file
  (and add a real `csv_parse.rs` format) once an actual Coinbase export is
  available.
