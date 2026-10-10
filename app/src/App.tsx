import { useCallback, useEffect, useState } from "react";
import { api, type Edition, type EditionInfo, type GuideVersion, type Options } from "./api";
import { clearMapCache } from "./components/WorldMap";
import { I18nProvider, LANGS, detectLang, useT, type Lang } from "./i18n";
import { AddonPage } from "./pages/AddonPage";
import { NewRoutePage, type WizardStart } from "./pages/NewRoutePage";
import { GuidesPage } from "./pages/GuidesPage";
import { SettingsPage } from "./pages/SettingsPage";
import { ContributePage } from "./pages/ContributePage";
import { UpdateBanner } from "./components/UpdateBanner";

const EDITION_LABELS: Record<Edition, string> = { forever: "Forever", classic: "Classic", tbc: "TBC" };

type Page = "plan" | "results" | "addon" | "contribute" | "settings";

const PAGES: { id: Page; label: string }[] = [
  { id: "plan", label: "Nouveau guide" },
  { id: "results", label: "Mes guides" },
  { id: "addon", label: "Addon" },
  { id: "contribute", label: "Contribuer" },
  { id: "settings", label: "Réglages" },
];

/** App language: the saved setting, else the system language. */
export default function App() {
  const [lang, setLang] = useState<Lang>(detectLang());
  useEffect(() => {
    api
      .getSettings()
      .then((s) => {
        const saved = LANGS.find((l) => l.key === s.language);
        if (saved) setLang(saved.key);
      })
      .catch(() => {});
  }, []);
  useEffect(() => {
    document.documentElement.lang = lang;
  }, [lang]);
  return (
    <I18nProvider lang={lang}>
      <Shell onLanguage={setLang} />
    </I18nProvider>
  );
}

function Shell({ onLanguage }: { onLanguage: (lang: Lang) => void }) {
  const t = useT();
  const [page, setPage] = useState<Page>("plan");
  const [options, setOptions] = useState<Options | null>(null);
  const [error, setError] = useState<string | null>(null);
  // The new route wizard restarts (key) with a configuration to edit or recompute.
  const [wizard, setWizard] = useState<{ key: number; start: WizardStart }>({ key: 0, start: { initial: null, rerun: false } });
  // A generation is running: the wizard keeps it (and its progress) until it ends.
  const [busy, setBusy] = useState(false);
  const openWizard = (start: WizardStart) => {
    if (!busy) setWizard((w) => ({ key: w.key + 1, start }));
    setPage("plan");
  };

  const load = useCallback(() => {
    api.getOptions().then(setOptions).catch((e) => setError(String(e)));
  }, []);
  useEffect(load, [load]);

  // Game version: switching reloads the options (races, classes, settings) and the guides.
  const [editions, setEditions] = useState<EditionInfo[]>([]);
  const [edition, setEdition] = useState<Edition | null>(null);
  useEffect(() => {
    api.listEditions().then(setEditions).catch(() => {});
    api.getSettings().then((s) => setEdition(s.edition)).catch(() => {});
  }, []);
  const switchEdition = async (e: Edition) => {
    if (busy || e === edition) return;
    await api.setEdition(e);
    clearMapCache();
    setEdition(e);
    setOptions(null);
    setError(null);
    load();
    setWizard((w) => ({ key: w.key + 1, start: { initial: null, rerun: false } }));
  };

  return (
    <div className="app">
      <nav className="sidebar">
        <div className="brand">
          <span className="logo">★</span> Factoruide
        </div>
        <div className="edition-switch" role="radiogroup" aria-label={t("Version du jeu")}>
          {editions.map((e) => (
            <button
              key={e.key}
              role="radio"
              aria-checked={e.key === edition}
              className={e.key === edition ? "active" : ""}
              disabled={!e.available || busy}
              title={!e.available ? t("Pas encore disponible") : busy ? t("Génération en cours") : e.name + (e.installed ? "" : " — " + t("client non trouvé dans le dossier WoW"))}
              onClick={() => switchEdition(e.key)}
            >
              {EDITION_LABELS[e.key]}
            </button>
          ))}
        </div>
        {PAGES.map((p) => (
          <button
            key={p.id}
            className={page === p.id ? "active" : ""}
            onClick={() => (p.id === "plan" && page === "plan" ? openWizard({ initial: null, rerun: false }) : setPage(p.id))}
          >
            {t(p.label)}
            {p.id === "plan" && busy && <span className="spinner" role="status" aria-label={t("Génération en cours")} title={t("Génération en cours")} />}
          </button>
        ))}
        <div className="sidebar-footer">{t("Pexing optimisé")}</div>
      </nav>
      <div className="content">
        <UpdateBanner />
        {/* Always mounted (hidden on other pages) so that a running generation goes on showing. */}
        {options && (
          <div hidden={page !== "plan"}>
            <NewRoutePage key={wizard.key} options={options} start={wizard.start} onBusy={setBusy} />
          </div>
        )}
        {page === "plan" &&
          !options &&
          (error ? (
            <div className="error">
              {t("Impossible de charger les données : {error}", { error })}
            </div>
          ) : (
            <div className="empty">{t("Chargement…")}</div>
          ))}
        {page === "results" && (
          <GuidesPage
            key={edition ?? ""}
            options={options}
            onRerun={(v: GuideVersion) => openWizard({ initial: v.request, rerun: true })}
            onEdit={(v: GuideVersion) => openWizard({ initial: v.request, rerun: false })}
          />
        )}
        {page === "addon" && <AddonPage />}
        {page === "contribute" && <ContributePage key={edition ?? ""} onSettings={() => setPage("settings")} />}
        {page === "settings" && <SettingsPage onLanguage={onLanguage} />}
      </div>
    </div>
  );
}
