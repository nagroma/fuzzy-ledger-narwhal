import { useState } from "react";
import { api } from "../../api";
import { useHoldings } from "./useHoldings";

/// TRANSFER: between two of the end user's own holdings, non-taxable. Basis and
/// acquisition date travel with the coins automatically (ledger::fold_ledger
/// handles that server-side) — this form just records which two holdings
/// and how much moved.
export function TransferForm({ onAdded }: { onAdded: () => void }) {
  const { holdings } = useHoldings();
  const [fromHoldingId, setFromHoldingId] = useState<number | null>(null);
  const [toHoldingId, setToHoldingId] = useState<number | null>(null);
  const [asset, setAsset] = useState("");
  const [qty, setQty] = useState("");
  const [eventAt, setEventAt] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  // useHoldings() defaults holdingId to the first holding; this form needs
  // two independent selections instead, seeded once the list loads.
  if (fromHoldingId === null && holdings.length > 0) setFromHoldingId(holdings[0].id);
  if (toHoldingId === null && holdings.length > 1) setToHoldingId(holdings[1].id);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (fromHoldingId === null || toHoldingId === null) return;
    setError(null);
    setSubmitting(true);
    try {
      await api.addTransferEntry({
        from_holding_id: fromHoldingId,
        to_holding_id: toHoldingId,
        asset,
        qty,
        event_at: eventAt,
      });
      setAsset("");
      setQty("");
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
      <h2>Record a transfer between your own holdings</h2>
      <div className="ledger-form-grid">
        <label>
          From holding
          <select value={fromHoldingId ?? ""} onChange={(e) => setFromHoldingId(Number(e.target.value))} required>
            {holdings.map((h) => (
              <option key={h.id} value={h.id}>
                {h.display_name}
              </option>
            ))}
          </select>
        </label>

        <label>
          To holding
          <select value={toHoldingId ?? ""} onChange={(e) => setToHoldingId(Number(e.target.value))} required>
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
          <input value={qty} onChange={(e) => setQty(e.target.value)} placeholder="0.5" inputMode="decimal" required />
        </label>

        <label>
          Date
          <input type="date" value={eventAt} onChange={(e) => setEventAt(e.target.value)} required />
        </label>
      </div>
      {fromHoldingId !== null && fromHoldingId === toHoldingId && (
        <p className="form-error">Source and destination holdings must differ.</p>
      )}

      <button type="submit" disabled={submitting || fromHoldingId === null || toHoldingId === null || fromHoldingId === toHoldingId}>
        {submitting ? "Adding…" : "Add entry"}
      </button>

      {error && <p className="form-error">{error}</p>}
    </form>
  );
}
