import { useEffect, useState } from "react";
import { api, type Settings } from "../api";
import { useT } from "../i18n";

type Option =
  | { key: string; kind: "bool"; label: string; help?: string; default: boolean }
  | { key: string; kind: "number"; label: string; help?: string; default: number; min: number; max: number; step: number }
  | { key: string; kind: "select"; label: string; help?: string; default: string; options: { value: string; label: string }[] };

/** Mirror of `FG.defaults` in addon/Factoruide/Settings.lua. */
const OPTIONS: { title: string; options: Option[] }[] = [
  {
    title: "Guide",
    options: [
      { key: "guideShown", kind: "bool", label: "Afficher le guide", default: true },
      {
        key: "uiTheme",
        kind: "select",
        label: "Apparence",
        help: "Automatique : le style d'ElvUI, EllesmereUI ou SpartanUI s'ils sont installés, sinon celui du jeu. Pris en compte après /reload.",
        default: "auto",
        options: [
          { value: "auto", label: "Automatique" },
          { value: "blizzard", label: "Style du jeu" },
          { value: "dark", label: "Sombre" },
        ],
      },
      { key: "nextLines", kind: "number", label: "Lignes de la boîte « Ensuite »", help: "0 la masque.", default: 5, min: 0, max: 15, step: 1 },
      { key: "scale", kind: "number", label: "Taille des fenêtres", default: 1, min: 0.6, max: 1.6, step: 0.05 },
      { key: "lockFrames", kind: "bool", label: "Verrouiller les fenêtres", default: false },
      { key: "waypoints", kind: "bool", label: "Flèche vers l'étape en cours", help: "Waypoint natif du jeu.", default: true },
      { key: "arrowShown", kind: "bool", label: "Grande flèche de guidage", help: "Tourne avec le personnage : verte quand l'étape est devant, rouge quand elle est derrière, distance en dessous. Déplaçable.", default: true },
    ],
  },
  {
    title: "Suivi des quêtes",
    options: [
      { key: "trackerShown", kind: "bool", label: "Afficher le suivi des quêtes Factoruide", help: "Quêtes en cours triées selon le guide, objectifs, à rendre, objets de quête.", default: true },
      { key: "replaceTracker", kind: "bool", label: "Masquer le suivi de quêtes de Blizzard", default: true },
      { key: "trackerMaxQuests", kind: "number", label: "Nombre max de quêtes affichées", default: 25, min: 1, max: 35, step: 1 },
      { key: "trackerAll", kind: "bool", label: "Afficher tout le journal", help: "Sinon, seulement les quêtes en cours dans le guide (une étape parmi les prochaines).", default: false },
      { key: "trackerWindow", kind: "number", label: "Étapes à venir prises en compte", help: "Une quête est « en cours » si une de ses étapes est dans ces prochaines étapes.", default: 15, min: 3, max: 60, step: 1 },
      { key: "professionsShown", kind: "bool", label: "Afficher la progression des métiers", help: "Compétence actuelle face au palier prévu pour votre niveau.", default: true },
    ],
  },
  {
    title: "Quêtes",
    options: [
      { key: "autoAccept", kind: "bool", label: "Accepter les quêtes automatiquement", help: "Maintenir Maj en parlant au PNJ pour désactiver.", default: true },
      { key: "routeQuestsOnly", kind: "bool", label: "Seulement les quêtes du guide", help: "Les autres quêtes ne sont pas acceptées automatiquement ; vous pouvez toujours les prendre à la main.", default: true },
      { key: "autoTurnIn", kind: "bool", label: "Rendre les quêtes automatiquement", default: true },
      { key: "autoReward", kind: "bool", label: "Choisir la récompense (meilleur prix de vente)", help: "Celle prévue par le guide d'abord, sinon la meilleure amélioration d'équipement.", default: false },
    ],
  },
  {
    title: "Macro de ciblage",
    options: [
      { key: "targetMacro", kind: "bool", label: "Mettre à jour la macro « FG Cible »", help: "Elle cible les mobs ou le PNJ de l'étape en cours. Tapez /fg macro en jeu, posez-la sur une barre et associez-lui un raccourci.", default: true },
      { key: "targetMark", kind: "bool", label: "Marquer la cible d'un crâne", default: true },
    ],
  },
  {
    title: "Divers",
    options: [
      { key: "fastLoot", kind: "bool", label: "Loot rapide", help: "Quand le loot automatique du jeu est actif.", default: true },
      { key: "autoTrain", kind: "bool", label: "Apprendre les sorts chez l'entraîneur de classe", default: true },
      { key: "trainUseful", kind: "bool", label: "Seulement les sorts utiles au leveling", help: "Ceux que le guide compte pour la puissance du personnage ; les autres restent à acheter soi-même.", default: true },
      { key: "autoFly", kind: "bool", label: "Prendre le vol de l'étape automatiquement", help: "À l'ouverture de la carte des vols (Maj pour choisir soi-même).", default: true },
      { key: "questItemButton", kind: "bool", label: "Bouton de l'objet de quête", help: "À côté du guide ; raccourci dans les touches du jeu (Factoruide).", default: true },
      { key: "mapPins", kind: "bool", label: "Prochaines étapes sur la carte du monde", default: true },
      { key: "mapPinsCount", kind: "number", label: "Étapes affichées sur la carte", default: 15, min: 3, max: 40, step: 1 },
      { key: "levelTiming", kind: "bool", label: "Temps de jeu comparé au guide", help: "Avance ou retard sur le temps prévu, dans l'en-tête du guide ; /fg time pour le détail.", default: true },
    ],
  },
  {
    title: "Marchands",
    options: [
      { key: "autoSellJunk", kind: "bool", label: "Vendre les objets gris", help: "Maintenir Maj en ouvrant le marchand pour désactiver.", default: true },
      { key: "autoRepair", kind: "bool", label: "Réparer automatiquement", default: true },
      { key: "autoSupplies", kind: "bool", label: "Acheter nourriture, eau et munitions", help: "Les meilleures de votre niveau chez le marchand, dans la limite de 30 % de votre or.", default: false },
      { key: "suppliesFood", kind: "number", label: "Nourriture à garder", default: 20, min: 0, max: 100, step: 5 },
      { key: "suppliesWater", kind: "number", label: "Eau à garder", help: "Classes à mana seulement.", default: 20, min: 0, max: 100, step: 5 },
      { key: "suppliesAmmo", kind: "number", label: "Munitions à garder", help: "Chasseurs : flèches ou balles selon l'arme.", default: 1000, min: 0, max: 3000, step: 200 },
    ],
  },
  {
    title: "Équipement et talents",
    options: [
      { key: "gearAdvisor", kind: "bool", label: "Conseiller d'équipement", help: "Amélioration indiquée dans les infobulles et sur les récompenses ; le choix automatique de récompense la prend en premier.", default: true },
      { key: "talentAdvisor", kind: "bool", label: "Talent conseillé", help: "Plan de leveling de la classe, affiché sous le guide quand il reste des points.", default: true },
      { key: "autoLearnTalents", kind: "bool", label: "Apprendre les talents automatiquement", default: false },
    ],
  },
  {
    title: "Groupe et sécurité",
    options: [
      { key: "partySync", kind: "bool", label: "Partager la progression avec le groupe", help: "Les membres avec Factoruide voient vos objectifs à côté des leurs.", default: true },
      { key: "safetyAlerts", kind: "bool", label: "Alertes de danger", help: "Cible élite ou bien plus haute que vous, vie basse ; objectifs élites signalés dans le guide.", default: true },
    ],
  },
];

export function AddonPage() {
  const t = useT();
  const [settings, setSettings] = useState<Settings | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.getSettings().then(setSettings).catch((e) => setError(String(e)));
  }, []);
  if (!settings) return error ? <div className="error">{error}</div> : null;

  const value = (o: Option) => settings.addon[o.key] ?? o.default;
  const update = (key: string, v: boolean | number | string) => setSettings({ ...settings, addon: { ...settings.addon, [key]: v } });

  const save = async (install: boolean) => {
    setError(null);
    try {
      // Every option is written so the addon always gets explicit values.
      const addon: Record<string, boolean | number | string> = {};
      for (const g of OPTIONS) for (const o of g.options) addon[o.key] = value(o);
      const next = { ...settings, addon };
      await api.setSettings(next);
      setSettings(next);
      setMessage(install ? t("Guides dans l'addon : {n} — faites /reload en jeu", { n: await api.installAddon() }) : t("Réglages enregistrés"));
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className="addon-page">
      <div className="actions">
        <button className="primary" onClick={() => save(true)}>
          {t("Enregistrer et installer l'addon")}
        </button>
        <button onClick={() => save(false)}>{t("Enregistrer")}</button>
      </div>
      {message && <div className="success">{message}</div>}
      {error && <div className="error">{error}</div>}
      <div className="addon-grid">
        {OPTIONS.map((g) => (
          <section className="panel" key={g.title}>
            <h3>{t(g.title)}</h3>
            {g.options.map((o) =>
              o.kind === "bool" ? (
                <label className="field field-bool" key={o.key}>
                  <input type="checkbox" checked={Boolean(value(o))} onChange={(e) => update(o.key, e.target.checked)} />
                  <span>{t(o.label)}</span>
                  <span />
                  {o.help && <small>{t(o.help)}</small>}
                </label>
              ) : o.kind === "select" ? (
                <label className="field" key={o.key}>
                  <span>{t(o.label)}</span>
                  <select value={String(value(o))} onChange={(e) => update(o.key, e.target.value)}>
                    {o.options.map((opt) => (
                      <option key={opt.value} value={opt.value}>
                        {t(opt.label)}
                      </option>
                    ))}
                  </select>
                  {o.help && <small>{t(o.help)}</small>}
                </label>
              ) : (
                <label className="field" key={o.key}>
                  <span>{t(o.label)}</span>
                  <input type="number" min={o.min} max={o.max} step={o.step} value={Number(value(o))} onChange={(e) => update(o.key, Number(e.target.value))} />
                  {o.help && <small>{t(o.help)}</small>}
                </label>
              ),
            )}
          </section>
        ))}
      </div>
    </div>
  );
}
