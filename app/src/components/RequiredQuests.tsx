import { useEffect, useState } from "react";
import { api, type QuestChoice } from "../api";
import { useT } from "../i18n";
import { requiredQuest } from "../requiredQuest";

interface Props {
  race: string;
  klass: string;
  locale: string | null;
  quests: (number | string)[];
  onChange: (quests: (number | string)[]) => void;
}

/** Must-have quests: searched by name, zone, level or ID among those of the race and class, kept by ID. */
export function RequiredQuests({ race, klass, locale, quests, onChange }: Props) {
  const t = useT();
  const [query, setQuery] = useState("");
  const [found, setFound] = useState<QuestChoice[]>([]);
  // Names of the quests chosen, for their chips.
  const [names, setNames] = useState<Map<number, QuestChoice>>(new Map());

  useEffect(() => {
    const q = query.trim();
    if (!q) return;
    let live = true;
    const timer = window.setTimeout(() => {
      api
        .questChoices(race, klass, q, locale)
        .then((list) => live && setFound(list))
        .catch(() => live && setFound([]));
    }, 200);
    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, [query, race, klass, locale]);

  // Quests chosen before (a guide made again): named once.
  useEffect(() => {
    for (const id of quests) {
      if (typeof id !== "number" || names.has(id)) continue;
      api
        .questChoices(race, klass, String(id), locale)
        .then((list) => {
          const q = list.find((c) => c.id === id);
          if (q) setNames((m) => new Map(m).set(id, q));
        })
        .catch(() => {});
    }
  }, [quests, names, race, klass, locale]);

  const add = (quest: number | string, choice?: QuestChoice) => {
    if (choice) setNames((m) => new Map(m).set(choice.id, choice));
    if (!quests.includes(quest)) onChange([...quests, quest]);
    setQuery("");
  };
  // Nothing searched: nothing listed (the last answer is kept for the next keystroke).
  const shown = query.trim() ? found : [];
  const label = (quest: number | string) => (typeof quest === "number" ? (names.get(quest)?.name ?? `#${quest}`) : quest);

  return (
    <section className="panel">
      <h3>{t("Quêtes indispensables")}</h3>
      <p className="muted">{t("Cherchez une quête par son nom, sa zone, son niveau ou son ID (« barrens 20 »). Ses prérequis sont inclus et le guide la termine, même après le niveau cible.")}</p>
      <div className="chips">
        {quests.map((quest) => (
          <button key={`${typeof quest}:${quest}`} className="chip" title={typeof quest === "number" ? `#${quest}` : undefined} onClick={() => onChange(quests.filter((q) => q !== quest))}>
            {label(quest)} ×
          </button>
        ))}
      </div>
      <input
        type="search"
        aria-label={t("Nom, zone, niveau ou ID")}
        placeholder={t("Nom, zone, niveau ou ID")}
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={(e) => {
          if (e.key !== "Enter" || !query.trim()) return;
          const first = shown[0];
          if (first) add(first.id, first);
          else add(requiredQuest(query));
        }}
      />
      {shown.length > 0 && (
        <ul className="pick-list quest-choices">
          {shown.map((q) => (
            <li key={q.id}>
              <button className="link" disabled={quests.includes(q.id)} onClick={() => add(q.id, q)}>
                {q.name}
              </button>
              <span className="muted">
                {" "}
                · {t("niveau {level}", { level: q.level })}
                {q.zone ? ` · ${q.zone}` : ""} · #{q.id}
              </span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
