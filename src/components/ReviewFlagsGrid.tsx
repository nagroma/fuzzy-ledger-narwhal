import type { ReviewFlagDto } from "../api";

const REASON_LABELS: Record<string, string> = {
  CROSS_ENTITY_TRANSFER: "Transfer crosses entities (e.g. Personal ↔ Business LLC)",
  FEE_BURN_UNSETTLED: "Network fee burn — tax treatment unsettled",
  DUST_INCOME: "Unsolicited dust — basis defaulted to $0",
};

export function ReviewFlagsGrid({ flags }: { flags: ReviewFlagDto[] }) {
  if (flags.length === 0) {
    return <p className="empty-state">Nothing flagged for CPA review.</p>;
  }

  return (
    <table className="grid">
      <thead>
        <tr>
          <th>Entry ID</th>
          <th>Reason</th>
        </tr>
      </thead>
      <tbody>
        {flags.map((f, i) => (
          <tr key={`${f.entry_id}-${i}`}>
            <td>{f.entry_id}</td>
            <td>{REASON_LABELS[f.reason] ?? f.reason}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
