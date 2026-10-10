import { useCallback, useEffect, useState } from "react";
import { api, type ContributionPreview } from "../api";
import { useDate, useT } from "../i18n";

/** Send this computer's quest cache and addon recordings to consolidate the quest database. */
export function ContributePage({ onSettings }: { onSettings: () => void }) {
  const t = useT();
  const date = useDate();
  const [preview, setPreview] = useState<ContributionPreview | null>(null);
  const [sending, setSending] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    api.contributionPreview().then(setPreview).catch((e) => setError(String(e)));
  }, []);
  useEffect(load, [load]);

  const send = async () => {
    setSending(true);
    setError(null);
    setMessage(null);
    try {
      const status = await api.sendContribution();
      setMessage(status === "duplicate" ? t("Ces données ont déjà été envoyées.") : t("Merci ! Vos données ont été envoyées."));
      load();
    } catch (e) {
      setError(String(e) === "rate_limited" ? t("Trop d'envois récents : réessayez plus tard.") : t("Le serveur a refusé l'envoi : {error}", { error: String(e) }));
    } finally {
      setSending(false);
    }
  };

  const empty = preview && preview.quests === 0 && preview.events === 0;
  return (
    <section className="panel contribute">
      <h3>{t("Contribuer")}</h3>
      <p>
        {t(
          "Les quêtes que votre jeu a vues (son cache de quêtes) et ce que l'addon a enregistré (quêtes prises et rendues, PNJ et positions) complètent la base de quêtes. Plus il y a de joueurs, plus les guides sont justes.",
        )}
      </p>
      <p className="muted">
        {t("Rien d'autre n'est envoyé : pas de compte, pas de nom de personnage (remplacé par un identifiant anonyme).")}{" "}
        {t("Un identifiant aléatoire de cette installation accompagne l'envoi, pour compter les joueurs différents.")}
      </p>
      <p className="muted">
        {t("Chaque envoi est comparé aux données déjà sûres : seules les informations confirmées par plusieurs joueurs sont intégrées.")}
      </p>
      {preview && (
        <div className="contribute-figures">
          <div className="tile">
            <span>{t("Quêtes du cache")}</span>
            <strong>{preview.quests}</strong>
            <small>{preview.locales.join(", ") || "—"}</small>
          </div>
          <div className="tile">
            <span>{t("Enregistrements de l'addon")}</span>
            <strong>{preview.events}</strong>
            <small>{t("{n} personnages", { n: preview.characters })}</small>
          </div>
          <div className="tile">
            <span>{t("Taille")}</span>
            <strong>{Math.max(1, Math.round(preview.size / 1024))} Ko</strong>
            <small>{preview.client_dir}</small>
          </div>
        </div>
      )}
      {preview && !preview.found && <div className="error">{t("Dossier du jeu introuvable : vérifiez le dossier World of Warcraft dans les Réglages.")}</div>}
      {preview && !preview.url && (
        <div className="error">
          {t("Aucun serveur de contribution configuré.")}{" "}
          <button className="link" onClick={onSettings}>
            {t("Réglages")}
          </button>
        </div>
      )}
      <div className="actions">
        <button className="primary" disabled={!preview?.url || !!empty || sending} onClick={send}>
          {sending ? t("Envoi…") : t("Envoyer mes données")}
        </button>
        {preview?.last_sent && <span className="muted">{t("Dernier envoi : {date}", { date: date(preview.last_sent) })}</span>}
      </div>
      {empty && <p className="muted">{t("Rien à envoyer pour l'instant : jouez avec l'addon puis revenez.")}</p>}
      {message && <div className="success">{message}</div>}
      {error && <div className="error">{error}</div>}
    </section>
  );
}
