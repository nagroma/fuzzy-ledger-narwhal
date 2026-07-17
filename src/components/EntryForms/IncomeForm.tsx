import { useState } from "react";
import { api } from "../../api";
import { useHoldings } from "./useHoldings";

/// INCOME: an acquisition at fair-market value on the day received (staking
/// reward, airdrop, etc.) — same shape as a BUY, different entry_type.
export function IncomeForm({ onAdded }: { onAdded: () => void }) {
  const { holdings, holdingId, setHoldingId } = useHoldings();
  const [asset, setAsset] = useState("");
  const [qty, setQty] = useState("");
  const [costBasisPerUnit, setCostBasisPerUnit] = useState("");
  const [eventAt, setEventAt] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (holdingId === null) return;
    setError(null);
    setSubmitting(true);
    try {
      await api.addIncomeEntry({
        holding_id: holdingId,
        asset,
        qty,
        cost_basis_per_unit: costBasisPerUnit,
        event_at: eventAt,
      });
      setAsset("");
      setQty("");
      setCostBasisPerUnit("");
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
      <h2>Record income (staking, airdrop, etc.)</h2>
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
          <input value={qty} onChange={(e) => setQty(e.target.value)} placeholder="10" inputMode="decimal" required />
        </label>

        <label>
          Fair market value / unit ($)
          <input
            value={costBasisPerUnit}
            onChange={(e) => setCostBasisPerUnit(e.target.value)}
            placeholder="0.55"
            inputMode="decimal"
            required
          />
        </label>

        <label>
          Date received
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
