import type { LotDto } from "../api";

export function LotsGrid({ lots }: { lots: LotDto[] }) {
  if (lots.length === 0) {
    return <p className="empty-state">No lots yet — add a BUY entry.</p>;
  }

  return (
    <table className="grid">
      <thead>
        <tr>
          <th>Lot ID</th>
          <th>Holding</th>
          <th>Asset</th>
          <th>Qty remaining</th>
          <th>Cost basis / unit</th>
          <th>Acquired</th>
        </tr>
      </thead>
      <tbody>
        {lots.map((l) => (
          <tr key={l.lot_id}>
            <td>{l.lot_id}</td>
            <td>{l.holding_name}</td>
            <td>{l.asset}</td>
            <td>{l.qty_remaining}</td>
            <td>{l.cost_basis_per_unit}</td>
            <td>{l.acquired_at.slice(0, 10)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
