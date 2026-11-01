import { useEffect, useState } from "react";
import { api, type SetupReport, type Step } from "../api";
import { gameLocale, useLang, useT } from "../i18n";

const NAME_KEY = "factoruide.issueName";
const MAX_MESSAGE = 4000;
const MAX_NAME = 40;
const NONE: number[] = [];

interface Props {
  /** The guide version the report is about (none: another problem). */
  guide?: { guide: string; version: number; steps: Step[] };
  /** Indices of the guide's steps pointed at, checked in its step table. */
  selected?: number[];
  onUnselect?: (index: number) => void;
  onClose?: () => void;
  /** Sent: the form is done. */
  onSent?: () => void;
}

/** Report a problem to the Factoruide server: a message, the steps of a guide, a name to thank. */
export function IssueForm({ guide, selected = NONE, onUnselect, onClose, onSent }: Props) {
  const t = useT();
  const lang = useLang();
  const [message, setMessage] = useState("");
  const [name, setName] = useState(() => localStorage.getItem(NAME_KEY) ?? "");
  const [sending, setSending] = useState(false);
  const [done, setDone] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // How the game and the addon are set up, joined unless unchecked.
  const [withSetup, setWithSetup] = useState(true);
  const [setup, setSetup] = useState<SetupReport | null>(null);
  useEffect(() => {
    api
      .setupReport()
      .then(setSetup)
      .catch(() => setSetup(null));
  }, []);

  const send = async () => {
    setSending(true);
    setError(null);
    localStorage.setItem(NAME_KEY, name.trim());
    try {
      const status = await api.sendIssue({
        kind: guide ? "guide" : "other",
        message,
        name: name.trim() || undefined,
        language: lang,
        guide: guide && { guide: guide.guide, version: guide.version, steps: [...selected].sort((a, b) => a - b), locale: gameLocale(lang) },
        setup: withSetup && setup !== null,
      });
      setDone(status === "already_received" ? t("Ce signalement a déjà été envoyé.") : t("Merci ! Votre signalement a été envoyé."));
      setMessage("");
      onSent?.();
    } catch (e) {
      const reason = String(e);
      setError(reason === "rate_limited" ? t("Trop d'envois récents : réessayez plus tard.") : t("Le serveur a refusé l'envoi : {error}", { error: reason }));
    } finally {
      setSending(false);
    }
  };

  if (done)
    return (
      <section className="panel issue-form">
        <div className="success">{done}</div>
        {onClose && <button onClick={onClose}>{t("Fermer")}</button>}
      </section>
    );

  return (
    <section className="panel issue-form">
      <h3>{t("Signaler un problème")}</h3>
      {guide && (
        <div className="issue-steps">
          <p className="muted">
            {selected.length
              ? t("Étapes jointes ({n}) :", { n: selected.length })
              : t("Cochez dans le tableau des étapes celles qui posent problème (facultatif).")}
          </p>
          {selected.length > 0 && (
            <ul>
              {[...selected]
                .sort((a, b) => a - b)
                .map((i) => (
                  <li key={i}>
                    <span className="step-n">{i + 1}</span> {guide.steps[i]?.text}
                    {onUnselect && (
                      <button className="link" aria-label={t("Retirer")} title={t("Retirer")} onClick={() => onUnselect(i)}>
                        ×
                      </button>
                    )}
                  </li>
                ))}
            </ul>
          )}
        </div>
      )}
      <label className="field">
        <span>{t("Message")}</span>
        <textarea
          rows={5}
          maxLength={MAX_MESSAGE}
          value={message}
          onChange={(e) => setMessage(e.target.value)}
          placeholder={guide ? t("Ce qui ne va pas : PNJ absent, quête impossible, trajet étrange…") : t("Décrivez le problème : ce que vous faisiez, ce qui s'est passé.")}
        />
      </label>
      <label className="field">
        <span>{t("Pseudo (facultatif)")}</span>
        <input maxLength={MAX_NAME} value={name} onChange={(e) => setName(e.target.value)} placeholder={t("Pour vous remercier dans les notes de version")} />
      </label>
      {setup && (
        <div className="issue-setup">
          <label className="field-bool">
            <input type="checkbox" checked={withSetup} onChange={(e) => setWithSetup(e.target.checked)} />
            <span>{t("Joindre l'état du jeu et de l'addon")}</span>
          </label>
          <details>
            <summary>{t("Ce qui est joint")}</summary>
            <ul>
              <li>
                {t("Dossier du jeu")} : {setup.wow_dir}
              </li>
              <li>
                {t("Versions du jeu trouvées")} : {setup.clients.join(", ") || "—"}
              </li>
              {setup.problem && (
                <li>
                  {t("Problème")} : {setup.problem}
                </li>
              )}
              <li>
                {t("Guide en jeu")} : {setup.guide_addon}
              </li>
              <li>
                {t("Addon installé")} : {setup.addon_version ?? "—"}
                {setup.addon_version && !setup.addon_complete ? ` (${t("incomplet")})` : ""}
              </li>
              <li>
                {t("Vu en jeu")} : {setup.seen_version ? t("version {version}, il y a {days} jours", { version: setup.seen_version, days: setup.seen_days_ago ?? 0 }) : "—"}
                {setup.not_loaded ? ` (${t("pas chargé par le jeu depuis l'installation")})` : ""}
              </li>
              <li>
                {t("Personnages où il est désactivé")} : {setup.disabled}
              </li>
            </ul>
          </details>
        </div>
      )}
      <p className="muted">
        {guide
          ? t("Sont joints : votre message, votre pseudo s'il est rempli, la configuration complète du guide et ses étapes (sans le nom du personnage), les versions de l'app et des données. N'écrivez rien de personnel dans le message.")
          : t("Sont joints : votre message, votre pseudo s'il est rempli, les versions de l'app et du jeu, votre langue et votre système. N'écrivez rien de personnel dans le message.")}
      </p>
      {error && <div className="error">{error}</div>}
      <div className="actions">
        <button className="primary" disabled={sending || !message.trim()} onClick={send}>
          {sending ? t("Envoi…") : t("Envoyer")}
        </button>
        {onClose && (
          <button onClick={onClose} disabled={sending}>
            {t("Annuler")}
          </button>
        )}
      </div>
    </section>
  );
}
