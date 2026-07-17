import type { LedgerEntryDto } from "../api";

export function EntriesGrid({ entries }: { entries: LedgerEntryDto[] }) {
  if (entries.length === 0) {
    return <p className="empty-state">No ledger entries yet.</p>;
  }

  return (
    <table className="grid">
      <thead>
        <tr>
          <th>ID</th>
          <th>Holding</th>
          <th>Type</th>
          <th>Asset</th>
          <th>Qty</th>
          <th>Cost basis / unit</th>
          <th>Proceeds</th>
          <th>Date</th>
        </tr>
      </thead>
      <tbody>
        {entries.map((e) => (
          <tr key={e.id}>
            <td>{e.id}</td>
            <td>{e.holding_name}</td>
            <td>{e.entry_type}</td>
            <td>{e.asset}</td>
            <td>{e.qty}</td>
            <td>{e.cost_basis_per_unit ?? "—"}</td>
            <td>{e.proceeds_total ?? "—"}</td>
            <td>{e.event_at.slice(0, 10)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
