import { useEffect, useState } from "react";
import { api, type StagingRowDto } from "../api";

// Every unresolved transfer leg, across every import so far — the other leg
// may be in a different file, a different venue's export, or may not exist
// yet. These never touch the ledger until matched (automatically, on the
// next commit) or paired here by hand.
export function PendingTransfers() {
  const [rows, setRows] = useState<StagingRowDto[]>([]);
  const [selectedOut, setSelectedOut] = useState<number | null>(null);
  const [selectedIn, setSelectedIn] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = () => {
    api.listPendingTransfers().then(setRows).catch((err) => setError(String(err)));
  };

  useEffect(refresh, []);

  const outLegs = rows.filter((r) => r.transfer_leg === "OUT");
  const inLegs = rows.filter((r) => r.transfer_leg === "IN");

  async function connect() {
    if (selectedOut === null || selectedIn === null) return;
    setError(null);
    try {
      await api.resolvePendingTransferPair(selectedOut, selectedIn);
      setSelectedOut(null);
      setSelectedIn(null);
      refresh();
    } catch (err) {
      setError(String(err));
    }
  }

  if (rows.length === 0) {
    return <p className="empty-state">No transfer legs waiting on a pair.</p>;
  }

  return (
    <div>
      <p className="form-note">
        These are one-sided transfer legs (a deposit/withdrawal/subaccount-transfer row) whose other side
        hasn't been imported or matched yet. They stay out of the ledger until resolved.
      </p>
      {error && <p className="form-error">{error}</p>}
      <div className="ledger-form-grid">
        <div>
          <h3>Leaving a holding</h3>
          <ul>
            {outLegs.map((r) => (
              <li key={r.id}>
                <label>
                  <input
                    type="radio"
                    name="out-leg"
                    checked={selectedOut === r.id}
                    onChange={() => setSelectedOut(r.id)}
                  />{" "}
                  {r.qty} {r.asset} on {r.event_at?.slice(0, 10)} (batch {r.import_batch_id})
                </label>
              </li>
            ))}
          </ul>
        </div>
        <div>
          <h3>Arriving at a holding</h3>
          <ul>
            {inLegs.map((r) => (
              <li key={r.id}>
                <label>
                  <input
                    type="radio"
                    name="in-leg"
                    checked={selectedIn === r.id}
                    onChange={() => setSelectedIn(r.id)}
                  />{" "}
                  {r.qty} {r.asset} on {r.event_at?.slice(0, 10)} (batch {r.import_batch_id})
                </label>
              </li>
            ))}
          </ul>
        </div>
      </div>
      <button type="button" onClick={connect} disabled={selectedOut === null || selectedIn === null}>
        Connect as one transfer
      </button>
    </div>
  );
}
