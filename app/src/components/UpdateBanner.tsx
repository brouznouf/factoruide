import { useCallback, useEffect, useRef, useState } from "react";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { api } from "../api";
import { useT } from "../i18n";

type Step = { kind: "idle" | "checking" | "current" | "installing" | "installed" } | { kind: "downloading"; percent: number | null } | { kind: "failed"; error: string };

interface Props {
  showCheck?: boolean;
  installBlocked?: boolean;
  beforeInstall?: () => boolean;
}

const allowInstall = () => true;

/** Startup checks for releases; an explicit Settings check also works in local builds. */
export function UpdateBanner({ showCheck = false, installBlocked = false, beforeInstall = allowInstall }: Props) {
  const t = useT();
  const [update, setUpdate] = useState<Update | null>(null);
  const [step, setStep] = useState<Step>({ kind: "idle" });
  const [local, setLocal] = useState(false);
  const locked = useRef(false);
  const manualChecks = useRef(0);
  const resource = useRef<Update | null>(null);

  const replaceUpdate = useCallback((next: Update | null) => {
    const previous = resource.current;
    resource.current = next;
    setUpdate(next);
    if (previous && previous !== next) void previous.close().catch(() => {});
  }, []);

  useEffect(() => {
    let cancelled = false;
    // A manual check takes precedence over a late startup result.
    api.updatesEnabled().then(async (enabled) => {
      if (cancelled) return;
      setLocal(!enabled);
      if (!enabled || manualChecks.current > 0) return;
      const next = await check({ timeout: 30000 });
      if (cancelled || manualChecks.current > 0) {
        if (next) await next.close();
        return;
      }
      replaceUpdate(next);
    }).catch(() => {});
    return () => {
      cancelled = true;
      if (resource.current) void resource.current.close().catch(() => {});
      resource.current = null;
    };
  }, [replaceUpdate]);

  const checkNow = async () => {
    if (locked.current) return;
    locked.current = true;
    manualChecks.current += 1;
    replaceUpdate(null);
    setStep({ kind: "checking" });
    try {
      setLocal(!(await api.updatesEnabled()));
      const next = await check({ timeout: 30000 });
      replaceUpdate(next);
      setStep({ kind: next ? "idle" : "current" });
    } catch (e) {
      setStep({ kind: "failed", error: String(e) });
    } finally {
      locked.current = false;
    }
  };

  const restart = async () => {
    try {
      await relaunch();
    } catch (e) {
      // Keep the installed state: retry restarting, never download/install twice.
      setRestartError(String(e));
    }
  };
  const [restartError, setRestartError] = useState<string | null>(null);
  const install = async () => {
    if (!update || locked.current || installBlocked || !beforeInstall()) return;
    if (!confirm(t(local
      ? "Installer la version officielle à la place de cette version locale et redémarrer ?"
      : "Installer la mise à jour et redémarrer Factoruide ?"))) return;
    locked.current = true;
    let total = 0;
    let received = 0;
    setStep({ kind: "downloading", percent: null });
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") total = event.data.contentLength ?? 0;
        if (event.event === "Progress") {
          received += event.data.chunkLength;
          setStep({ kind: "downloading", percent: total ? Math.min(100, Math.round((received / total) * 100)) : null });
        }
        if (event.event === "Finished") setStep({ kind: "installing" });
      });
      setStep({ kind: "installed" });
      await restart();
    } catch (e) {
      setStep({ kind: "failed", error: String(e) });
      locked.current = false;
    }
  };

  const working = ["checking", "downloading", "installing", "installed"].includes(step.kind);
  if (!showCheck && !update && step.kind !== "failed") return null;
  return (
    <section className="update-banner" aria-label={t("Mises à jour de l'application")}>
      <div className="update-text" role="status" aria-live="polite">
        <strong>{update ? t("Version {version} disponible", { version: update.version }) : t("Mises à jour de l'application")}</strong>
        {update?.body && <div className="update-notes">{update.body}</div>}
        {step.kind === "current" && <p>{t("Vous utilisez la dernière version.")}</p>}
        {step.kind === "checking" && <p>{t("Recherche de mises à jour…")}</p>}
        {step.kind === "failed" && <div className="error">{t("Échec de la mise à jour : {error}", { error: step.error })}</div>}
        {local && showCheck && <p>{t("Version locale : une mise à jour la remplacera par la version officielle.")}</p>}
        {installBlocked && update && <p>{t("Attendez la fin de la génération avant de mettre à jour.")}</p>}
        {restartError && <div className="error">{t("Redémarrage impossible : {error}", { error: restartError })}</div>}
      </div>
      {step.kind === "downloading" && <span>{step.percent === null ? t("Téléchargement…") : t("Téléchargement… {percent} %", { percent: step.percent })}</span>}
      {step.kind === "installing" && <span>{t("Installation…")}</span>}
      {step.kind === "installed" && <button onClick={restart}>{t("Redémarrer Factoruide")}</button>}
      {update && !working && <>
        <button className="primary" disabled={installBlocked} onClick={install}>{t("Mettre à jour")}</button>
        <button onClick={() => { replaceUpdate(null); setStep({ kind: "idle" }); }}>{t("Plus tard")}</button>
      </>}
      {showCheck && <button disabled={working} onClick={checkNow}>{t("Rechercher des mises à jour")}</button>}
    </section>
  );
}
