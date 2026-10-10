import { useMemo, useState } from "react";
import { useAreaName, useT } from "../i18n";

interface Zone {
  id: number;
  name: string;
  names?: Record<string, string>;
  continent: number;
}

interface Props {
  label: string;
  help?: string;
  zones: Zone[];
  selected: number[];
  onChange: (ids: number[]) => void;
}

const CONTINENTS: Record<number, string> = { 0: "Royaumes de l'Est", 1: "Kalimdor" };

/** Multi-select of zones with a search box; selected zones are shown as removable chips. */
export function ZonePicker({ label, help, zones, selected, onChange }: Props) {
  const t = useT();
  const areaName = useAreaName();
  const [query, setQuery] = useState("");
  const byId = useMemo(() => new Map(zones.map((z) => [z.id, z])), [zones]);
  const matches = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return [];
    const found = (z: Zone) => z.name.toLowerCase().includes(q) || areaName(z).toLowerCase().includes(q);
    return zones.filter((z) => found(z) && !selected.includes(z.id)).slice(0, 8);
  }, [query, zones, selected, areaName]);

  return (
    <div className="field">
      <span>{label}</span>
      <div className="chips">
        {selected.map((id) => (
          <button key={id} className="chip" onClick={() => onChange(selected.filter((s) => s !== id))} title={t("Retirer")}>
            {byId.has(id) ? areaName(byId.get(id)!) : id} ×
          </button>
        ))}
      </div>
      <input placeholder={t("Rechercher une zone…")} value={query} onChange={(e) => setQuery(e.target.value)} />
      {matches.length > 0 && (
        <ul className="suggestions">
          {matches.map((z) => (
            <li key={z.id}>
              <button
                className="link"
                onClick={() => {
                  onChange([...selected, z.id]);
                  setQuery("");
                }}
              >
                {areaName(z)} <small>{CONTINENTS[z.continent] ? t(CONTINENTS[z.continent]) : `continent ${z.continent}`}</small>
              </button>
            </li>
          ))}
        </ul>
      )}
      {help && <small>{help}</small>}
    </div>
  );
}
