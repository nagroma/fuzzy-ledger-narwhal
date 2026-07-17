# CryptoTracker

A Windows desktop crypto transaction ledger built with Tauri + Rust + SQLite
+ React/TS. It tracks cost-basis lots per holding (FIFO, Rev. Proc. 2024-28
compliant), supports the full range of crypto transaction types (buys,
sells, trades, transfers, gifts, income, network fee burns, unsolicited
dust), and imports CSV exports from custodians/exchanges with duplicate
detection — all producing a CPA-ready record, not a tax filing. Everything
lives in one SQLite file the end user owns and can hand to their CPA;
nothing is sent anywhere else.

See **[`user-guide.md`](user-guide.md)** for what the app does and how to
use it (end-user facing), and **[`architecture.md`](architecture.md)** for
how it's built and why (developer facing).

## Downloading and installing (no dev tools needed)

You don't need Rust, Node, or any of the build tooling below — that's only
for whoever's changing the code. To get the app itself:

1. Go to the [Releases page](https://github.com/nagroma/fuzzy-ledger-narwhal/releases).
2. Under the latest release, download **`cryptotracker_<version>_x64-setup.exe`**
   (or the `.msi` — either installs the same app; the `.exe` is slightly
   smaller and generally the easier double-click).
3. Run it. Windows may show a SmartScreen "unknown publisher" warning the
   first time (this isn't signed with a paid code-signing certificate) —
   click **More info → Run anyway**. That's expected for a small unsigned
   app, not a sign anything's wrong.
4. Launch **cryptotracker** from the Start Menu like any other app.

**Getting an update:** repeat the same steps with the newest release —
installing over the old version keeps your ledger data (it lives in your
Windows user profile, separate from the program files). See "Cleaning up"
below if you ever want to wipe the ledger and start fresh instead.

**Your data:** everything you enter stays in one file on your machine
(`%APPDATA%\com.andrewmo.cryptotracker\cryptotracker.sqlite`) — nothing is
sent anywhere. Back that file up the same way you'd back up any important
document.

## Project layout

```
src-tauri/
  migrations/0001_initial_schema.sql   the schema (see architecture.md)
  sample_data/                         obfuscated CSV fixtures for testing
  src/
    lots.rs        pure FIFO cost-basis engine (no tauri/rusqlite)
    ledger.rs      pure fold layer: ledger -> current lots/disposals/flags
    db.rs          opens the SQLite file, runs migrations + seed
    migrations.rs  versioned schema migration runner
    seed.rs        real venue/holding inventory (generic placeholder names —
                    see "Local names" below)
    models.rs       wire DTOs (ts-rs generates src/bindings/*.ts from these)
    commands.rs     the only file converting SQL rows <-> Decimal/DateTime
                    <-> wire strings; every #[tauri::command] lives here
    import/         CSV import: parse -> stage -> reconcile -> commit
src/
  api.ts                     every invoke() call, typed
  App.tsx                    tab state, top-level layout
  components/                grids, the venue tree, import UI
  components/EntryForms/     one form per transaction type
```

## Local names for testing

This repo is public, so real personal/company names never appear in
committed code — `seed.rs` uses generic placeholders (`Business LLC`,
`Tangem — Card A`, etc.). If you're testing locally and want to see real
names instead, copy `src-tauri/local_seed_overrides.example.json` to
`src-tauri/local_seed_overrides.json` (gitignored, never committed) and fill
in real values — delete `cryptotracker.sqlite` and relaunch to reseed.

## Prerequisites

1. **Visual Studio Build Tools**, "Desktop development with C++" workload —
   Rust on Windows needs the MSVC linker. Slow, multi-GB, one-time.
2. **Rust** — https://rustup.rs
3. **Node 18+**
4. **WebView2** — already on Windows 11.

## Day to day (the build/debug loop)

```bash
# Fast inner loop: no UI, no webview, exercises the whole domain
cd src-tauri && cargo test
```

`cargo test` runs every hand-computed engine test (lot FIFO, ledger fold,
CSV parsing/staging) *and* regenerates `src/bindings/*.ts` from the Rust
DTOs in `models.rs` via ts-rs. If you change a model's fields and forget to
re-run this, the frontend types go stale — worth running after every model
change, not just before a commit.

```bash
# Full app, hot-reloading: Rust recompiles on save, Vite hot-reloads the UI
npm run tauri dev
```

First run compiles every dependency (Tauri, rusqlite+bundled SQLite, ts-rs...)
— budget several minutes. Every run after is seconds, because only changed
crates recompile. The app's SQLite file lives at
`%APPDATA%\com.andrewmo.cryptotracker\cryptotracker.sqlite` — delete it to
reset to a fresh seeded DB.

```bash
# The installer — what you'd actually hand someone
npm run tauri build
# → src-tauri/target/release/bundle/msi/*.msi  (and /nsis/*.exe)
```

This is a release (optimized) build plus a bundling step, so it's slower than
`dev`. The output is a real Windows installer: double-click, done, no Rust or
Node required on the target machine — that's the entire reason this stack was
chosen over Python/PySide (packaging friction) or Electron (150MB, no native
decimal type).

## What to notice while reading the code

- **`lots.rs` and `ledger.rs` import nothing from `tauri` or `rusqlite`.**
  That's a rule in `CLAUDE.md`, not an accident — it's what keeps
  `cargo test` fast and the domain logic portable if the shell around it
  ever changes. Same goes for `import/csv_parse.rs` and `import/dedup.rs`.
- **Every quantity is `TEXT` in SQLite and `String` on the wire.** Grep for
  `f64` in `src-tauri/src` — you won't find one anywhere near money.
- **`state.db.lock()`** — the `Mutex<Connection>` in `AppState`
  (`lib.rs`). SQLite is single-writer anyway, so this isn't a bottleneck; it's
  Rust making a Python/C# dev's "just use a global connection" instinct
  honest by making you say so at every call site.
- **Lots are computed, not stored.** `list_lots` rebuilds the current-holdings
  view from `ledger_entries` on every call via `ledger::fold_ledger` — the
  ledger itself is the only source of truth.
- **`Result<T, String>` on every command** becomes a rejected JS promise.
  Validation errors (bad quantity, bad date) surface in the form without any
  extra plumbing.

See `architecture.md` for the full domain model (entities/venues/holdings/
addresses, the entry-type taxonomy, transfer-basis mechanics, CSV import and
duplicate/pending-transfer handling) and what's not built yet.

## Cleaning up: what survives an uninstall

Installers (both the `.msi` and the NSIS `.exe`) remove the program files,
the Start Menu shortcut, and the registry Uninstall entry. They deliberately
**do not** touch per-user data — that's standard Windows installer behavior,
not a bug — so two folders outlive an uninstall on any machine this gets
installed on:

| Path | What it is | Safe to delete? |
|---|---|---|
| `%APPDATA%\com.andrewmo.cryptotracker\cryptotracker.sqlite` | the ledger itself | Only if you don't want the data. This is the one thing a real user would want to *keep* across a reinstall. |
| `%LOCALAPPDATA%\com.andrewmo.cryptotracker\EBWebView\` | WebView2's cache/profile folder for this app (Chromium cache, GPU shader cache, crash dumps) | Yes, always. Pure cache, regenerated on next launch. |

Deleting both folders (`rmdir /s` on each, or just delete via File Explorer)
is a full reset — same as a fresh install, no ledger, no cache.

Note the `%APPDATA%`/`%LOCALAPPDATA%` folder is keyed by the app **identifier**
(`com.andrewmo.cryptotracker`, set in `tauri.conf.json`), not by install
location — this is why `npm run tauri dev` and the installed `.msi` both read
and write the same files.

## Abandoning this project entirely: what's left outside the project folder

Deleting the project folder does not touch anything below — all of it lives
elsewhere because Rust/Node/Tauri tooling is shared across every project on
the machine, not per-project. Split into "definitely just this project's
junk" and "shared toolchain — check before removing."

**Definitely safe, definitely just this project:**

- `%APPDATA%\com.andrewmo.cryptotracker\` and
  `%LOCALAPPDATA%\com.andrewmo.cryptotracker\` — the ledger DB and WebView2
  cache described above. Nothing else uses this identifier.
- If you ever ran the `.msi`/`.exe` installer: uninstall via *Settings →
  Apps* (or the registry Uninstall entry) first, *then* delete the two
  folders above — the uninstaller itself won't touch them.

**Shared toolchain — installed for this project, but other work may now
depend on it. Check before removing:**

| What | Where | Notes |
|---|---|---|
| Rust toolchain (`rustup`) | `%USERPROFILE%\.rustup` | `rustup self uninstall` removes this cleanly |
| Cargo (registry cache, installed bins) | `%USERPROFILE%\.cargo` | Removed by the same `rustup self uninstall` |
| Node.js | `C:\Program Files\nodejs` | Uninstall via *Settings → Apps* like any Windows program |
| npm global cache | `%LOCALAPPDATA%\npm-cache` | Safe to delete any time; npm rebuilds it |
| Visual Studio Build Tools (C++ workload) | via *Visual Studio Installer* | Needed by *any* Rust project that links native code on Windows, not just this one |
| Tauri's cached bundler tools (WiX, NSIS) | `%LOCALAPPDATA%\tauri\` | Only used when running `tauri build`; harmless to delete, re-downloads next time a Tauri project bundles |

**Not installed by this project, don't touch:** WebView2 — it ships with
Windows 11 itself.

If you abandon the project and want the toolchain gone too: `rustup self
uninstall`, uninstall Node.js and Visual Studio Build Tools from *Settings →
Apps*, then delete `%LOCALAPPDATA%\tauri\`. That's everything this project
caused to exist outside the project folder.
