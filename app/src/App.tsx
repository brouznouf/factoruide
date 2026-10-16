import { useCallback, useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { api, type Edition, type EditionInfo, type GuideVersion, type Options, type PlanRequest } from "./api";
import { clearMapCache } from "./components/WorldMap";
import { I18nProvider, LANGS, detectLang, useT, type Lang } from "./i18n";
import { AddonPage } from "./pages/AddonPage";
import { NewRoutePage, type WizardStart } from "./pages/NewRoutePage";
import { GuidesPage, type OpenGuide } from "./pages/GuidesPage";
import { SettingsPage } from "./pages/SettingsPage";
import { CharactersPage } from "./pages/CharactersPage";
import { ChangelogPage } from "./pages/ChangelogPage";
import { ContributePage } from "./pages/ContributePage";
import { IssueForm } from "./components/IssueForm";
import { UpdateBanner } from "./components/UpdateBanner";

const EDITION_LABELS: Record<Edition, string> = { forever: "Forever", classic: "Classic", tbc: "TBC" };

type Page = "characters" | "plan" | "results" | "addon" | "contribute" | "settings" | "changelog" | "report";

const PAGES: { id: Page; label: string }[] = [
  { id: "plan", label: "Nouveau guide" },
  { id: "characters", label: "Personnages" },
  { id: "results", label: "Guides" },
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
  const main = useRef<HTMLElement>(null);
  useEffect(() => {
    if (page) main.current?.focus({ preventScroll: true });
  }, [page]);
  const [options, setOptions] = useState<Options | null>(null);
  const [error, setError] = useState<string | null>(null);
  // The new route wizard restarts (key) with a configuration to edit or recompute.
  const [wizard, setWizard] = useState<{ key: number; start: WizardStart }>({ key: 0, start: { initial: null, rerun: false } });
  // A generation is running: the wizard keeps it (and its progress) until it ends.
  const [busy, setBusy] = useState(false);
  // The guide "Mes guides" opens on (a generation just saved it), until the user navigates away.
  const [opened, setOpened] = useState<OpenGuide | null>(null);
  const [version, setVersion] = useState<string | null>(null);
  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => {});
  }, []);
  // The guide editor has changes not saved: leaving its page (another page, another game
  // version, a generation ending) asks first, as its own back button does.
  const unsaved = useRef(false);
  const setUnsaved = useCallback((u: boolean) => {
    unsaved.current = u;
  }, []);
  const away = (go: () => void) => {
    if (unsaved.current && !confirm(t("Quitter l'édition sans enregistrer ? Les modifications seront perdues."))) return;
    unsaved.current = false;
    go();
  };
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
  // Each game version keeps its new guide's configuration: coming back to it finds it as left.
  const drafts = useRef<Partial<Record<Edition, WizardStart>>>({});
  const draftCharacter = wizard.start.character;
  const saveDraft = useCallback(
    (request: PlanRequest) => {
      if (edition) drafts.current[edition] = { initial: request, rerun: false, character: draftCharacter };
    },
    [edition, draftCharacter],
  );
  const switchEdition = async (e: Edition) => {
    if (busy || e === edition) return;
    await api.setEdition(e);
    clearMapCache();
    setEdition(e);
    setOptions(null);
    setError(null);
    load();
    setWizard((w) => ({ key: w.key + 1, start: drafts.current[e] ?? { initial: null, rerun: false } }));
  };

  return (
    <div className="app">
      <nav className="sidebar" aria-label={t("Menu")}>
        <div className="brand">
          <span className="logo">★</span> Factoruide
        </div>
        <div className="sidebar-label" aria-hidden>
          {t("Version du jeu")}
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
              onClick={() => away(() => switchEdition(e.key))}
            >
              {EDITION_LABELS[e.key]}
            </button>
          ))}
        </div>
        {PAGES.map((p) => (
          <button
            key={p.id}
            className={page === p.id ? "active" : ""}
            onClick={() =>
              away(() => {
                setOpened(null);
                if (p.id === "plan" && page === "plan") openWizard({ initial: null, rerun: false });
                else setPage(p.id);
              })
            }
          >
            {t(p.label)}
            {p.id === "plan" && busy && <span className="spinner" role="status" aria-label={t("Génération en cours")} title={t("Génération en cours")} />}
          </button>
        ))}
        <div className="sidebar-footer">
          <button className={page === "changelog" ? "active" : ""} onClick={() => away(() => setPage("changelog"))}>
            {t("Nouveautés")}
          </button>
          <button className={page === "report" ? "active" : ""} onClick={() => away(() => setPage("report"))}>
            {t("Signaler un problème")}
          </button>
          {version && <span>Factoruide {version}</span>}
        </div>
      </nav>
      {/* Focused on each page change: the keyboard scrolls it and a screen reader starts there. */}
      <main className="content" ref={main} tabIndex={-1}>
        <UpdateBanner />
        {/* Always mounted (hidden on other pages) so that a running generation goes on showing. */}
        {options && (
          <div hidden={page !== "plan"}>
            <NewRoutePage
              key={wizard.key}
              options={options}
              start={wizard.start}
              onBusy={setBusy}
              onDraft={saveDraft}
              onDone={(meta, notice) => {
                // Started from a character's block: the character follows its new guide.
                const character = wizard.start.character;
                if (character) api.assignGuide(character, meta.guide).catch((e) => setError(String(e)));
                // Not over a guide being edited: the new guide waits in "Mes guides".
                if (unsaved.current) return;
                setOpened({ guide: meta.guide, version: meta.version, notice });
                setPage("results");
              }}
            />
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
        {page === "characters" && (
          <CharactersPage
            key={edition ?? ""}
            options={options}
            editionName={edition ? EDITION_LABELS[edition] : ""}
            onAddon={() => setPage("addon")}
            onOpenGuide={(guide, shown) => {
              setOpened({ guide, version: shown, notice: null });
              setPage("results");
            }}
            onNewGuide={(character) => openWizard({ initial: null, rerun: false, character })}
          />
        )}
        {page === "results" && (
          <GuidesPage
            key={`${edition ?? ""}-${opened?.guide ?? ""}-${opened?.version ?? ""}`}
            options={options}
            start={opened}
            onRerun={(v: GuideVersion) => openWizard({ initial: v.request, rerun: true })}
            onEdit={(v: GuideVersion) => openWizard({ initial: v.request, rerun: false })}
            onClone={(v: GuideVersion) => openWizard({ initial: { ...v.request, name: t("{name} (copie)", { name: v.request.name ?? v.meta.name }) }, rerun: false })}
            onUnsaved={setUnsaved}
          />
        )}
        {page === "addon" && <AddonPage />}
        {page === "changelog" && <ChangelogPage />}
        {page === "report" && (
          <div className="report-page">
            <div className="page-head">
              <h2>{t("Signaler un problème")}</h2>
            </div>
            <p className="muted">{t("Un problème dans un guide ? Ouvrez-le et cliquez sur « Signaler un problème » : vous pourrez y joindre les étapes concernées.")}</p>
            <IssueForm key={edition ?? ""} />
          </div>
        )}
        {page === "contribute" && <ContributePage edition={edition} />}
        {page === "settings" && <SettingsPage onLanguage={onLanguage} version={version} />}
      </main>
    </div>
  );
}
