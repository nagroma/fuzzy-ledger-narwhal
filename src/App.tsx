import { useCallback, useEffect, useState } from "react";
import { api, type LedgerEntryDto, type LotDto, type ReviewFlagDto } from "./api";
import { BuyForm } from "./components/EntryForms/BuyForm";
import { SellForm, GiftOutForm, FeeBurnForm } from "./components/EntryForms/DisposalEntryForm";
import { TradeForm } from "./components/EntryForms/TradeForm";
import { TransferForm } from "./components/EntryForms/TransferForm";
import { IncomeForm } from "./components/EntryForms/IncomeForm";
import { DustInForm } from "./components/EntryForms/DustInForm";
import { EntriesGrid } from "./components/EntriesGrid";
import { LotsGrid } from "./components/LotsGrid";
import { ReviewFlagsGrid } from "./components/ReviewFlagsGrid";
import { VenueHierarchy } from "./components/VenueHierarchy";
import { ImportPreview } from "./components/ImportPreview";
import { PendingTransfers } from "./components/PendingTransfers";
import "./App.css";

type Tab = "entries" | "lots" | "review" | "venues" | "import" | "pending-transfers";

const ENTRY_FORMS = {
  BUY: { label: "Buy", Form: BuyForm },
  SELL: { label: "Sell", Form: SellForm },
  TRADE: { label: "Trade", Form: TradeForm },
  TRANSFER: { label: "Transfer", Form: TransferForm },
  GIFT_OUT: { label: "Gift", Form: GiftOutForm },
  INCOME: { label: "Income", Form: IncomeForm },
  FEE_BURN: { label: "Fee burn", Form: FeeBurnForm },
  DUST_IN: { label: "Dust", Form: DustInForm },
} as const;

type EntryFormKind = keyof typeof ENTRY_FORMS;

function App() {
  const [tab, setTab] = useState<Tab>("entries");
  const [entryFormKind, setEntryFormKind] = useState<EntryFormKind>("BUY");
  const [entries, setEntries] = useState<LedgerEntryDto[]>([]);
  const [lots, setLots] = useState<LotDto[]>([]);
  const [reviewFlags, setReviewFlags] = useState<ReviewFlagDto[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    Promise.all([api.listLedgerEntries(), api.listLots(), api.listReviewFlags()])
      .then(([e, l, f]) => {
        setEntries(e);
        setLots(l);
        setReviewFlags(f);
        setLoadError(null);
      })
      .catch((err) => setLoadError(String(err)));
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const ActiveForm = ENTRY_FORMS[entryFormKind].Form;

  return (
    <main className="container">
      <h1>CryptoTracker — ledger</h1>

      <div className="tabs">
        {(Object.keys(ENTRY_FORMS) as EntryFormKind[]).map((kind) => (
          <button
            key={kind}
            className={entryFormKind === kind ? "tab active" : "tab"}
            onClick={() => setEntryFormKind(kind)}
          >
            {ENTRY_FORMS[kind].label}
          </button>
        ))}
      </div>

      <ActiveForm onAdded={refresh} />

      {loadError && <p className="form-error">{loadError}</p>}

      <div className="tabs">
        <button
          className={tab === "entries" ? "tab active" : "tab"}
          onClick={() => setTab("entries")}
        >
          Ledger entries ({entries.length})
        </button>
        <button
          className={tab === "lots" ? "tab active" : "tab"}
          onClick={() => setTab("lots")}
        >
          Lots ({lots.length})
        </button>
        <button
          className={tab === "review" ? "tab active" : "tab"}
          onClick={() => setTab("review")}
        >
          CPA review ({reviewFlags.length})
        </button>
        <button
          className={tab === "venues" ? "tab active" : "tab"}
          onClick={() => setTab("venues")}
        >
          Venues
        </button>
        <button
          className={tab === "import" ? "tab active" : "tab"}
          onClick={() => setTab("import")}
        >
          Import CSV
        </button>
        <button
          className={tab === "pending-transfers" ? "tab active" : "tab"}
          onClick={() => setTab("pending-transfers")}
        >
          Pending transfers
        </button>
      </div>

      {tab === "entries" && <EntriesGrid entries={entries} />}
      {tab === "lots" && <LotsGrid lots={lots} />}
      {tab === "review" && <ReviewFlagsGrid flags={reviewFlags} />}
      {tab === "venues" && <VenueHierarchy />}
      {tab === "import" && <ImportPreview />}
      {tab === "pending-transfers" && <PendingTransfers />}
    </main>
  );
}

export default App;
