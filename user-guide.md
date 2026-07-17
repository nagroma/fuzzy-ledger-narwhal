# CryptoTracker — User Guide

A plain-language guide to what this app does and how to use it. If you're
looking for how the code is built, see `architecture.md` instead — this doc
is about using the app, not building it.

**What this app is not:** a tax filer. It keeps a clean, CPA-ready record of
every crypto transaction and works out cost basis correctly — but it doesn't
decide what's taxable or fill out any tax form for you. Where the tax
treatment of something is genuinely unsettled, the app flags it for your CPA
rather than guessing.

## The structure: Venue → Holding → Address

Everything you enter belongs to a **holding** — the smallest bucket that has
its own cost basis. Holdings are grouped under **venues**:

- A **venue** is a place you access or hold crypto through: an exchange
  (like Coinbase), a custodian (a firm that holds assets on your behalf),
  or a self-custody hardware wallet card. **Each physical hardware wallet
  card is its own venue** — if you have two cards, they're two separate
  venues, even if they're the same brand.
- A **holding** is an account within a venue. Most venues only have one
  holding (your personal login), so the app just shows the venue's name and
  hides this extra layer. Some venues — a custodian holding both your
  personal money and your business's money, for instance — have more than
  one holding, and each one is tracked completely separately: money in one
  never mixes with money in another, even though they're "at the same
  place."
- An **address** is the actual on-chain address for one asset within a
  holding — shown alongside the asset symbol (e.g. "BTC") so you can
  visually confirm which address a transaction refers to. Public addresses
  aren't sensitive information (anyone can see them on the blockchain
  already) — but this app never asks for, and never stores, a private key
  or seed phrase. Don't ever enter one.

Right now there's no screen for adding/editing venues, holdings, or
addresses yourself — that inventory is set up ahead of time. If you need a
new one added, that's a request to whoever maintains the app for you.

## Entering transactions

Each tab across the top of the entry form is a different kind of
transaction. Pick the one that matches what actually happened:

| Tab | What it means |
|---|---|
| **Buy** | You bought crypto with cash. |
| **Sell** | You sold crypto for cash. |
| **Trade** | You swapped one crypto for another directly (no cash in between) — this still counts as a taxable event, same as a sale. |
| **Transfer** | You moved crypto between two of your *own* holdings (e.g. from an exchange to a hardware wallet). This is **not** a taxable event — you still own it, it just moved. |
| **Gift** | You gave crypto to someone else (e.g. a family member) with nothing expected in return. |
| **Income** | You received crypto as a reward — staking rewards, an airdrop, etc. |
| **Fee burn** | A network destroyed a small amount of crypto as a transaction fee (some networks work this way). Whether this is deductible/reportable is genuinely unsettled, so this always gets flagged for your CPA rather than assumed either way. |
| **Dust** | You received a tiny, unsolicited amount of crypto you didn't ask for. Defaults to $0 cost basis and always gets flagged for your CPA — the app doesn't assume it's ordinary income or nothing at all. |

For **Buy**, **Income**, and **Dust**, you're recording what something cost
(its "cost basis") when you got it. For **Sell**, **Trade**, and **Gift**,
you're recording what you got when you disposed of it (or, for a gift,
simply that you gave it away). **Transfer** doesn't ask for a price at all —
the app automatically carries over the original cost and purchase date from
wherever the coins came from, since that's what actually matters for taxes,
not the transfer date.

## The three main views

- **Ledger entries** — every transaction you've entered, in order. This is
  the permanent record; nothing here ever gets edited or deleted, only
  added to (if you made a mistake, ask whoever maintains the app about
  correcting it, rather than trying to edit the database directly).
- **Lots** — what you currently hold, broken down by holding and by
  purchase (a "lot" is one batch of an asset acquired together, e.g. "0.5
  BTC bought on this date at this price"). This updates automatically as
  you buy, sell, and transfer — you never edit it directly.
- **CPA review** — anything the app has deliberately flagged rather than
  decided on its own: a transfer that crossed between two different
  tax-relevant owners (e.g. personal money moving to a business holding),
  every fee burn, and every dust receipt. None of these are wrong or
  broken — they're exactly the cases where a computer program shouldn't be
  the one making the call.

## Importing from a CSV

If a venue can export your transaction history as a CSV file, you can
import it instead of typing everything in by hand:

1. Open the **Import CSV** tab, pick the file format (the specific export
   format from that venue) and which holding the file belongs to, then
   choose the file.
2. **Preview** shows you what was found: how many transactions are brand
   new, how many are already in your ledger (safe to skip), how many look
   like possible duplicates (these need you to look and decide), and how
   many are one side of a transfer waiting to be matched up with its other
   side (see below).
3. Go through the list and accept or reject anything that needs a
   decision — new transactions are usually safe to accept as-is.
4. **Commit** writes everything you accepted into your permanent ledger.

### Transfers split across files

Sometimes a transfer's two sides show up in two completely different
files — say, money leaving one venue in one export and arriving at another
venue in a separate export (possibly imported weeks apart, or one side not
imported yet at all). The app handles this by holding one-sided transfers in
a waiting area — the **Pending transfers** tab — instead of guessing or
dropping them. Every time you commit an import, the app automatically
checks whether any waiting transfer now has a match (same asset, same
amount) anywhere else you've imported, and connects them automatically if
so. If two sides never turn up as an automatic match, you can connect them
yourself from the Pending transfers tab. Until a transfer is matched, it
doesn't affect anything else — your other totals and views work normally.

## Your data

Everything lives in one file on your computer — nothing is sent anywhere
else. Back it up the same way you'd back up any other important document.
If you ever reinstall or update the app, your data stays put; it lives
separately from the program itself.
