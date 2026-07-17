import { useEffect, useState } from "react";
import { api } from "../api";
import type { VenueHierarchyDto } from "../bindings/VenueHierarchyDto";

// Read-only Venue > Holding (collapsed when it's the venue's only, default
// holding) > Address view. No management UI yet — the real inventory is
// entered by hand in seed.rs until that exists.
export function VenueHierarchy() {
  const [venues, setVenues] = useState<VenueHierarchyDto[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .listVenueHierarchy()
      .then(setVenues)
      .catch((err) => setError(String(err)));
  }, []);

  if (error) return <p className="form-error">{error}</p>;
  if (venues.length === 0) return <p className="empty-state">No venues yet.</p>;

  return (
    <div className="venue-hierarchy">
      {venues.map((v) => (
        <div key={v.venue_id} className="venue-card">
          <h3>
            {v.venue_name} <span className="venue-type">{v.venue_type}</span>
          </h3>
          <ul>
            {v.holdings.map((h) => (
              <li key={h.holding_id}>
                {h.is_default_holding ? (
                  <span>{h.entity_name}</span>
                ) : (
                  <span>
                    {h.holding_name} <span className="venue-type">({h.entity_name})</span>
                    {h.account_identifier ? ` — ${h.account_identifier}` : ""}
                  </span>
                )}
                {h.addresses.length > 0 && (
                  <ul>
                    {h.addresses.map((a) => (
                      <li key={a.id} title={a.address}>
                        {a.asset}: {a.address.length > 12 ? `${a.address.slice(0, 6)}…${a.address.slice(-4)}` : a.address}
                      </li>
                    ))}
                  </ul>
                )}
              </li>
            ))}
          </ul>
        </div>
      ))}
    </div>
  );
}
