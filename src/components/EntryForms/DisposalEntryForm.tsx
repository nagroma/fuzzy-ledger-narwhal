import { useState } from "react";
import { api, type NewDisposalEntry } from "../../api";
import { useHoldings } from "./useHoldings";

type ProceedsMode =
  | { kind: "input" } // SELL: user enters what was received
  | { kind: "zero"; note: string }; // GIFT_OUT / FEE_BURN: always $0 proceeds

/// Shared shape for SELL, GIFT_OUT, and FEE_BURN — all three are a disposal
/// of `qty` units of `asset` out of `holding_id`, differing only in whether
/// proceeds are entered (SELL) or fixed at $0 (GIFT_OUT, FEE_BURN, both
/// non-cash dispositions).
export function DisposalEntryForm({
  title,
  submit,
  proceedsMode,
  onAdded,
}: {
  title: string;
  submit: (entry: NewDisposalEntry) => Promise<unknown>;
  proceedsMode: ProceedsMode;
  onAdded: () => void;
}) {
  const { holdings, holdingId, setHoldingId } = useHoldings();
  const [asset, setAsset] = useState("");
  const [qty, setQty] = useState("");
  const [proceedsTotal, setProceedsTotal] = useState("");
  const [eventAt, setEventAt] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (holdingId === null) return;
    setError(null);
    setSubmitting(true);
    try {
      await submit({
        holding_id: holdingId,
        asset,
        qty,
        proceeds_total: proceedsMode.kind === "zero" ? "0" : proceedsTotal,
        event_at: eventAt,
      });
      setAsset("");
      setQty("");
      setProceedsTotal("");
      setEventAt("");
      onAdded();
    } catch (err) {
      setError(String(err));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <form className="ledger-form" onSubmit={handleSubmit}>
      <h2>{title}</h2>
      <div className="ledger-form-grid">
        <label>
          Holding
          <select
            value={holdingId ?? ""}
            onChange={(e) => setHoldingId(Number(e.target.value))}
            required
          >
            {holdings.map((h) => (
              <option key={h.id} value={h.id}>
                {h.display_name}
              </option>
            ))}
          </select>
        </label>

        <label>
          Asset
          <input value={asset} onChange={(e) => setAsset(e.target.value)} placeholder="BTC" required />
        </label>

        <label>
          Quantity
          <input
            value={qty}
            onChange={(e) => setQty(e.target.value)}
            placeholder="0.5"
            inputMode="decimal"
            required
          />
        </label>

        {proceedsMode.kind === "input" ? (
          <label>
            Proceeds received ($)
            <input
              value={proceedsTotal}
              onChange={(e) => setProceedsTotal(e.target.value)}
              placeholder="50000"
              inputMode="decimal"
              required
            />
          </label>
        ) : (
          <p className="form-note">{proceedsMode.note}</p>
        )}

        <label>
          Date
          <input type="date" value={eventAt} onChange={(e) => setEventAt(e.target.value)} required />
        </label>
      </div>

      <button type="submit" disabled={submitting || holdingId === null}>
        {submitting ? "Adding…" : "Add entry"}
      </button>

      {error && <p className="form-error">{error}</p>}
    </form>
  );
}

export function SellForm({ onAdded }: { onAdded: () => void }) {
  return (
    <DisposalEntryForm
      title="Record a sale (SELL)"
      submit={api.addSellEntry}
      proceedsMode={{ kind: "input" }}
      onAdded={onAdded}
    />
  );
}

export function GiftOutForm({ onAdded }: { onAdded: () => void }) {
  return (
    <DisposalEntryForm
      title="Record a gift (GIFT_OUT)"
      submit={api.addGiftOutEntry}
      proceedsMode={{ kind: "zero", note: "Gifts are non-taxable — no proceeds are recorded." }}
      onAdded={onAdded}
    />
  );
}

export function FeeBurnForm({ onAdded }: { onAdded: () => void }) {
  return (
    <DisposalEntryForm
      title="Record a network fee burn (FEE_BURN)"
      submit={api.addFeeBurnEntry}
      proceedsMode={{
        kind: "zero",
        note: "In-kind network destruction — tax treatment unsettled, always flagged for your CPA.",
      }}
      onAdded={onAdded}
    />
  );
}
