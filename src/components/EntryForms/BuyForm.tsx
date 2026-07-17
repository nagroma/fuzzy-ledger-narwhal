import { useState } from "react";
import { api } from "../../api";
import { useHoldings } from "./useHoldings";

export function BuyForm({ onAdded }: { onAdded: () => void }) {
  const { holdings, holdingId, setHoldingId } = useHoldings();
  const [asset, setAsset] = useState("");
  const [qty, setQty] = useState("");
  const [costBasisPerUnit, setCostBasisPerUnit] = useState("");
  const [acquiredAt, setAcquiredAt] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (holdingId === null) return;
    setError(null);
    setSubmitting(true);
    try {
      await api.addLedgerEntry({
        holding_id: holdingId,
        asset,
        qty,
        cost_basis_per_unit: costBasisPerUnit,
        acquired_at: acquiredAt,
      });
      setAsset("");
      setQty("");
      setCostBasisPerUnit("");
      setAcquiredAt("");
      onAdded();
    } catch (err) {
      setError(String(err));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <form className="ledger-form" onSubmit={handleSubmit}>
      <h2>Record a purchase (BUY)</h2>
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
          <input
            value={asset}
            onChange={(e) => setAsset(e.target.value)}
            placeholder="BTC"
            required
          />
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

        <label>
          Cost basis / unit ($)
          <input
            value={costBasisPerUnit}
            onChange={(e) => setCostBasisPerUnit(e.target.value)}
            placeholder="20000"
            inputMode="decimal"
            required
          />
        </label>

        <label>
          Acquired on
          <input
            type="date"
            value={acquiredAt}
            onChange={(e) => setAcquiredAt(e.target.value)}
            required
          />
        </label>
      </div>

      <button type="submit" disabled={submitting || holdingId === null}>
        {submitting ? "Adding…" : "Add entry"}
      </button>

      {error && <p className="form-error">{error}</p>}
    </form>
  );
}
