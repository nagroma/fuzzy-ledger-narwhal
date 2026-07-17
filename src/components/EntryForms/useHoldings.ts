import { useEffect, useState } from "react";
import { api, type Holding } from "../../api";

/// Shared by every entry form: the holding picker's options.
export function useHoldings() {
  const [holdings, setHoldings] = useState<Holding[]>([]);
  const [holdingId, setHoldingId] = useState<number | null>(null);

  useEffect(() => {
    api.listHoldings().then((h) => {
      setHoldings(h);
      if (h.length > 0) setHoldingId(h[0].id);
    });
  }, []);

  return { holdings, holdingId, setHoldingId };
}
