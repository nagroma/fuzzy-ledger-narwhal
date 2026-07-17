import { useState } from "react";
import { api } from "../../api";
import { useHoldings } from "./useHoldings";

/// TRADE: crypto-to-crypto, a disposal and an acquisition at once, both legs
/// at the same holding and moment, linked by a shared trade_id (server-side).
/// `fmv_total` is the fair-market-value of what came back — it's both the
/// disposed asset's proceeds and the acquired asset's total cost basis.
export function TradeForm({ onAdded }: { onAdded: () => void }) {
  const { holdings, holdingId, setHoldingId } = useHoldings();
  const [assetOut, setAssetOut] = useState("");
  const [qtyOut, setQtyOut] = useState("");
  const [assetIn, setAssetIn] = useState("");
  const [qtyIn, setQtyIn] = useState("");
  const [fmvTotal, setFmvTotal] = useState("");
  const [eventAt, setEventAt] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (holdingId === null) return;
    setError(null);
    setSubmitting(true);
    try {
      await api.addTradeEntry({
        holding_id: holdingId,
        asset_out: assetOut,
        qty_out: qtyOut,
        asset_in: assetIn,
        qty_in: qtyIn,
        fmv_total: fmvTotal,
        event_at: eventAt,
      });
      setAssetOut("");
      setQtyOut("");
      setAssetIn("");
      setQtyIn("");
      setFmvTotal("");
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
      <h2>Record a crypto-to-crypto trade</h2>
      <div className="ledger-form-grid">
        <label>
          Holding
          <select value={holdingId ?? ""} onChange={(e) => setHoldingId(Number(e.target.value))} required>
            {holdings.map((h) => (
              <option key={h.id} value={h.id}>
                {h.display_name}
              </option>
            ))}
          </select>
        </label>

        <label>
          Asset given up
          <input value={assetOut} onChange={(e) => setAssetOut(e.target.value)} placeholder="BTC" required />
        </label>

        <label>
          Quantity given up
          <input value={qtyOut} onChange={(e) => setQtyOut(e.target.value)} placeholder="1" inputMode="decimal" required />
        </label>

        <label>
          Asset received
          <input value={assetIn} onChange={(e) => setAssetIn(e.target.value)} placeholder="ETH" required />
        </label>

        <label>
          Quantity received
          <input value={qtyIn} onChange={(e) => setQtyIn(e.target.value)} placeholder="15" inputMode="decimal" required />
        </label>

        <label>
          Fair market value of what was received ($, total)
          <input
            value={fmvTotal}
            onChange={(e) => setFmvTotal(e.target.value)}
            placeholder="45000"
            inputMode="decimal"
            required
          />
        </label>

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
