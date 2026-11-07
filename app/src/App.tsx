import { useCallback, useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { api, type Edition, type EditionInfo, type GuideVersion, type Options, type PlanRequest } from "./api";
import { clearMapCache } from "./components/WorldMap";
import { I18nProvider, LANGS, detectLang, useT, type Lang } from "./i18n";
import { WowBanner } from "./components/WowBanner";
import { Welcome } from "./components/Welcome";
import { NewRoutePage, type WizardStart } from "./pages/NewRoutePage";
import { GuidesPage, type OpenGuide } from "./pages/GuidesPage";
import { SettingsPage } from "./pages/SettingsPage";
import { ChangelogPage } from "./pages/ChangelogPage";
import { ContributePage } from "./pages/ContributePage";
import { HomePage } from "./pages/HomePage";
import { NeedHelpPage } from "./pages/NeedHelpPage";
import { DungeonPage } from "./pages/DungeonPage";
import { exampleGuide, hasExample } from "./example";
import { IssueForm } from "./components/IssueForm";
import { UpdateBanner } from "./components/UpdateBanner";
import { IS_WEB } from "./web/mode";
import { WebSidebarTools } from "./web/components/Sidebar";
import { ADDON_URL } from "./web/components/AddonInstall";
import logo from "./assets/logo.svg";

/** Where problems are reported from the web app (it sends nothing anywhere itself). */
const ISSUES_URL = "https://github.com/brouznouf/factoruide/issues";

const EDITION_LABELS: Record<Edition, string> = { forever: "Forever", classic: "Classic", tbc: "TBC" };

type Page = "home" | "plan" | "dungeon" | "results" | "contribute" | "help" | "settings" | "changelog" | "report";

const PAGES: { id: Page; label: string }[] = [
  { id: "home", label: "Accueil" },
  { id: "plan", label: "Nouveau guide" },
  { id: "dungeon", label: "Donjon" },
  { id: "results", label: "Guides" },
  // The addon is installed, updated and set up in game (CurseForge, its page in the game's options: /fg).
  // In a browser, nothing is sent anywhere and the settings are in the menu.
  ...(IS_WEB
    ? []
    : ([
        { id: "contribute", label: "Contribuer" },
        { id: "help", label: "Besoin d'aide" },
        { id: "settings", label: "Réglages" },
      ] as { id: Page; label: string }[])),
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
  const [page, setPage] = useState<Page>("home");
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
  // The addon comes from CurseForge: its page there.
  const openAddonSite = () => {
    api.openLink(ADDON_URL).catch(() => {});
  };
  // The example guide (WoW Forever): added to the guides the first time, then opened.
  const openExample = async () => {
    setOpened(await exampleGuide());
    setPage("results");
  };
  // The dungeon guide page, on a dungeon guide's configuration when one is edited.
  const [dungeonStart, setDungeonStart] = useState<{ key: number; initial: PlanRequest | null }>({ key: 0, initial: null });
  const openWizard = (start: WizardStart) => {
    if (start.initial?.dungeon != null && !start.rerun) {
      setDungeonStart((d) => ({ key: d.key + 1, initial: start.initial }));
      setPage("dungeon");
      return;
    }
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
  // The first launch's questions, while World of Warcraft was not found.
  const [welcome, setWelcome] = useState(false);
  useEffect(() => {
    api.listEditions().then(setEditions).catch(() => {});
    api.getSettings().then((s) => setEdition(s.edition)).catch(() => {});
    Promise.all([api.getSettings(), api.wowStatus()])
      // In a browser, no game folder to look for: the home page says how to start.
      .then(([s, w]) => setWelcome(!IS_WEB && !s.welcome_done && (w.problem === "missing" || w.problem === "no_game")))
      .catch(() => {});
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
      {welcome && (
        <Welcome
          onEdition={async (e) => {
            await switchEdition(e);
            api.listEditions().then(setEditions).catch(() => {});
          }}
          onDone={(addon) => {
            setWelcome(false);
            api.listEditions().then(setEditions).catch(() => {});
            if (addon) openAddonSite();
          }}
        />
      )}
      <nav className="sidebar" aria-label={t("Menu")}>
        <div className="brand">
          <img className="logo" src={logo} alt="" /> Factoruide
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
          {IS_WEB && <WebSidebarTools onLanguage={onLanguage} />}
          <button className={page === "changelog" ? "active" : ""} onClick={() => away(() => setPage("changelog"))}>
            {t("Nouveautés")}
          </button>
          {IS_WEB ? (
            <a className="sidebar-link" href={ISSUES_URL} target="_blank" rel="noopener">
              {t("Signaler un problème")}
            </a>
          ) : (
            <button className={page === "report" ? "active" : ""} onClick={() => away(() => setPage("report"))}>
              {t("Signaler un problème")}
            </button>
          )}
          {version && <span>Factoruide {version}</span>}
        </div>
      </nav>
      {/* Focused on each page change: the keyboard scrolls it and a screen reader starts there. */}
      <main className="content" ref={main} tabIndex={-1}>
        <UpdateBanner busy={busy} unsaved={unsaved} />
        <WowBanner editionName={editions.find((e) => e.key === edition)?.name ?? ""} onSettings={() => away(() => setPage("settings"))} onAddon={openAddonSite} />
        {/* Always mounted (hidden on other pages) so that a running generation goes on showing. */}
        {options && (
          <div hidden={page !== "plan"}>
            <NewRoutePage
              key={wizard.key}
              options={options}
              start={wizard.start}
              onBusy={setBusy}
              onDraft={saveDraft}
              onDungeon={(request) => openWizard({ initial: request, rerun: false })}
              onDone={(meta, notice, edit) => {
                // Not over a guide being edited: the new guide waits in "Mes guides".
                if (unsaved.current) return;
                setOpened({ guide: meta.guide, version: meta.version, notice, edit });
                setPage("results");
              }}
            />
          </div>
        )}
        {page === "plan" &&
          !options &&
          (error ? (
            <div className="error">
              {error.includes("not_isolated")
                ? t("Ce navigateur ne permet pas à Factoruide de calculer les guides (il lui faut les Web Workers à mémoire partagée) : essayez une version récente de Chrome, Edge, Firefox ou Safari, et rechargez la page.")
                : t("Impossible de charger les données : {error}", { error })}
            </div>
          ) : (
            <div className="empty">{t("Chargement…")}</div>
          ))}
        {page === "results" && (
          <GuidesPage
            key={`${edition ?? ""}-${opened?.guide ?? ""}-${opened?.version ?? ""}`}
            options={options}
            start={opened}
            onRerun={(v: GuideVersion) => openWizard({ initial: v.request, rerun: true })}
            onEdit={(v: GuideVersion) => openWizard({ initial: v.request, rerun: false })}
            onClone={(v: GuideVersion) => openWizard({ initial: { ...v.request, name: t("{name} (copie)", { name: v.request.name ?? v.meta.name }) }, rerun: false })}
            onUnsaved={setUnsaved}
            edition={edition}
            onExample={hasExample(edition) ? openExample : undefined}
          />
        )}
        {page === "home" && (
          <HomePage
            key={edition ?? ""}
            editionName={editions.find((e) => e.key === edition)?.name ?? ""}
            onNewGuide={() => openWizard({ initial: null, rerun: false })}
            onGuides={() => setPage("results")}
            onExample={hasExample(edition) ? openExample : undefined}
            onAddon={openAddonSite}
          />
        )}
        {page === "dungeon" &&
          (options ? (
            <DungeonPage
              key={`${edition ?? ""}-${dungeonStart.key}`}
              options={options}
              initial={dungeonStart.initial}
              busy={busy}
              onGenerate={(request) => {
                if (!busy) setWizard((w) => ({ key: w.key + 1, start: { initial: request, rerun: true } }));
                setPage("plan");
              }}
            />
          ) : (
            <div className="empty">{t("Chargement…")}</div>
          ))}
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
        {page === "help" && <NeedHelpPage key={edition ?? ""} />}
        {page === "settings" && <SettingsPage onLanguage={onLanguage} version={version} />}
      </main>
    </div>
  );
}
