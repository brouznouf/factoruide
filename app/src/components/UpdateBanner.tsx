import { useEffect, useState } from "react";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { api } from "../api";
import { useT } from "../i18n";

type Step = { kind: "idle" } | { kind: "downloading"; percent: number | null } | { kind: "installing" } | { kind: "failed"; error: string };

/** Offers the new version published on GitHub (checked once at startup), with its notes. */
export function UpdateBanner() {
  const t = useT();
  const [update, setUpdate] = useState<Update | null>(null);
  const [step, setStep] = useState<Step>({ kind: "idle" });

  useEffect(() => {
    // Published builds only; offline, or no release yet: nothing to offer.
    api
      .updatesEnabled()
      .then((enabled) => (enabled ? check() : null))
      .then(setUpdate)
      .catch(() => {});
  }, []);

  if (!update) return null;

  const install = async () => {
    let total = 0;
    let received = 0;
    setStep({ kind: "downloading", percent: null });
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") total = event.data.contentLength ?? 0;
        if (event.event === "Progress") {
          received += event.data.chunkLength;
          setStep({ kind: "downloading", percent: total ? Math.round((received / total) * 100) : null });
        }
        if (event.event === "Finished") setStep({ kind: "installing" });
      });
      // Windows closes the app to run the installer; elsewhere it starts again here.
      await relaunch();
    } catch (e) {
      setStep({ kind: "failed", error: String(e) });
    }
  };

  return (
    <div className="update-banner" role="status">
      <div className="update-text">
        <strong>{t("Version {version} disponible", { version: update.version })}</strong>
        {update.body && <div className="update-notes">{update.body}</div>}
        {step.kind === "failed" && <div className="error">{t("Échec de la mise à jour : {error}", { error: step.error })}</div>}
      </div>
      {step.kind === "downloading" && (
        <span>{step.percent === null ? t("Téléchargement…") : t("Téléchargement… {percent} %", { percent: step.percent })}</span>
      )}
      {step.kind === "installing" && <span>{t("Installation…")}</span>}
      {(step.kind === "idle" || step.kind === "failed") && (
        <>
          <button className="primary" onClick={install}>
            {t("Mettre à jour")}
          </button>
          <button onClick={() => setUpdate(null)}>{t("Plus tard")}</button>
        </>
      )}
    </div>
  );
}
