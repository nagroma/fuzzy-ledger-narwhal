import { useState } from "react";
import { api } from "../../api";
import { useHoldings } from "./useHoldings";

/// DUST_IN: de minimis unsolicited incoming. Always $0 cost basis by
/// convention (not user-entered) and always flagged for CPA review — no
/// proceeds/basis field here on purpose.
export function DustInForm({ onAdded }: { onAdded: () => void }) {
  const { holdings, holdingId, setHoldingId } = useHoldings();
  const [asset, setAsset] = useState("");
  const [qty, setQty] = useState("");
  const [eventAt, setEventAt] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (holdingId === null) return;
    setError(null);
    setSubmitting(true);
    try {
      await api.addDustInEntry({ holding_id: holdingId, asset, qty, event_at: eventAt });
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
      <h2>Record unsolicited dust (DUST_IN)</h2>
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
          Asset
          <input value={asset} onChange={(e) => setAsset(e.target.value)} placeholder="XRP" required />
        </label>

        <label>
          Quantity
          <input
            value={qty}
            onChange={(e) => setQty(e.target.value)}
            placeholder="0.001"
            inputMode="decimal"
            required
          />
        </label>

        <label>
          Date received
          <input type="date" value={eventAt} onChange={(e) => setEventAt(e.target.value)} required />
        </label>
      </div>
      <p className="form-note">Defaults to $0 cost basis and is always flagged for your CPA's review.</p>

      <button type="submit" disabled={submitting || holdingId === null}>
        {submitting ? "Adding…" : "Add entry"}
      </button>

      {error && <p className="form-error">{error}</p>}
    </form>
  );
}
