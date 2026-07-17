import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type ImportPreviewDto, type StagingRowDto } from "../api";
import { useHoldings } from "./EntryForms/useHoldings";

const CLASSIFICATION_LABELS: Record<string, string> = {
  NEW: "New",
  MATCHED_EXISTING: "Already in the ledger",
  NEAR_DUPLICATE: "Possible duplicate — needs a decision",
  PENDING_TRANSFER: "Transfer leg — pending pair",
};

export function ImportPreview() {
  const { holdings, holdingId, setHoldingId } = useHoldings();
  const [format, setFormat] = useState("DWP_ANCHORAGE");
  const [filePath, setFilePath] = useState<string | null>(null);
  const [preview, setPreview] = useState<ImportPreviewDto | null>(null);
  const [rows, setRows] = useState<StagingRowDto[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [committing, setCommitting] = useState(false);
  const [commitResult, setCommitResult] = useState<string | null>(null);

  async function pickFile() {
    setError(null);
    const selected = await open({
      multiple: false,
      filters: [{ name: "CSV", extensions: ["csv"] }],
    });
    if (typeof selected === "string") setFilePath(selected);
  }

  async function runPreview() {
    if (!filePath || holdingId === null) return;
    setError(null);
    setCommitResult(null);
    try {
      // Venue is implied by the holding for now — venue_id isn't otherwise
      // used by import_csv_preview beyond recording it on the batch.
      const holding = holdings.find((h) => h.id === holdingId);
      const venueId = holding?.id ?? holdingId;
      const result = await api.importCsvPreview({ filePath, venueId, holdingId, format });
      setPreview(result);
      setRows(await api.listStagingRows(result.batch_id));
    } catch (err) {
      setError(String(err));
    }
  }

  async function decide(rowId: number, decision: string) {
    await api.resolveStagingRow(rowId, decision);
    if (preview) setRows(await api.listStagingRows(preview.batch_id));
  }

  async function commit() {
    if (!preview) return;
    setCommitting(true);
    try {
      const summary = await api.importCsvCommit(preview.batch_id);
      setCommitResult(
        `Committed ${summary.ledger_entries_committed} ledger entr${summary.ledger_entries_committed === 1 ? "y" : "ies"}, ` +
          `${summary.cash_activity_entries_committed} cash-activity entr${summary.cash_activity_entries_committed === 1 ? "y" : "ies"}, ` +
          `auto-resolved ${summary.transfers_auto_resolved} transfer pair${summary.transfers_auto_resolved === 1 ? "" : "s"}.`
      );
      setRows(await api.listStagingRows(preview.batch_id));
    } catch (err) {
      setError(String(err));
    } finally {
      setCommitting(false);
    }
  }

  return (
    <div>
      <div className="ledger-form">
        <h2>Import a CSV</h2>
        <div className="ledger-form-grid">
          <label>
            Format
            <select value={format} onChange={(e) => setFormat(e.target.value)}>
              <option value="DWP_ANCHORAGE">DWP / Anchorage export</option>
              <option value="GENERIC_PLACEHOLDER">Generic (placeholder, no confirmed format yet)</option>
            </select>
          </label>
          <label>
            Holding this file belongs to
            <select value={holdingId ?? ""} onChange={(e) => setHoldingId(Number(e.target.value))}>
              {holdings.map((h) => (
                <option key={h.id} value={h.id}>
                  {h.display_name}
                </option>
              ))}
            </select>
          </label>
        </div>
        <button type="button" onClick={pickFile}>
          {filePath ? filePath.split(/[\\/]/).pop() : "Choose CSV file…"}
        </button>{" "}
        <button type="button" onClick={runPreview} disabled={!filePath || holdingId === null}>
          Preview
        </button>
        {error && <p className="form-error">{error}</p>}
      </div>

      {preview && (
        <div className="ledger-form">
          <h2>Preview: {preview.total_rows} rows read</h2>
          <ul>
            <li>{preview.new_count} new</li>
            <li>{preview.matched_existing_count} already in the ledger</li>
            <li>{preview.near_duplicate_count} possible duplicates — need a decision</li>
            <li>{preview.pending_transfer_count} transfer legs pending a pair</li>
            <li>{preview.cash_activity_count} cash-activity rows (fees, etc.)</li>
          </ul>
          {preview.parse_errors.length > 0 && (
            <div>
              <p className="form-error">{preview.parse_errors.length} row(s) couldn't be parsed:</p>
              <ul>
                {preview.parse_errors.map((e, i) => (
                  <li key={i} className="form-error">
                    {e}
                  </li>
                ))}
              </ul>
            </div>
          )}

          <table className="grid">
            <thead>
              <tr>
                <th>Classification</th>
                <th>Type</th>
                <th>Asset</th>
                <th>Qty</th>
                <th>Date</th>
                <th>Decision</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr key={r.id}>
                  <td>{CLASSIFICATION_LABELS[r.classification] ?? r.classification}</td>
                  <td>{r.entry_type ?? "—"}</td>
                  <td>{r.asset ?? "—"}</td>
                  <td>{r.qty ?? "—"}</td>
                  <td>{r.event_at?.slice(0, 10) ?? "—"}</td>
                  <td>
                    {r.classification === "PENDING_TRANSFER" ? (
                      <span className="form-note">resolved on commit / see Pending transfers tab</span>
                    ) : r.decision ? (
                      r.decision
                    ) : (
                      <>
                        <button type="button" onClick={() => decide(r.id, "ACCEPT")}>
                          Accept
                        </button>{" "}
                        <button type="button" onClick={() => decide(r.id, "REJECT")}>
                          Reject
                        </button>
                      </>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>

          <button type="button" onClick={commit} disabled={committing} style={{ marginTop: "1rem" }}>
            {committing ? "Committing…" : "Commit accepted rows"}
          </button>
          {commitResult && <p className="form-note">{commitResult}</p>}
        </div>
      )}
    </div>
  );
}
