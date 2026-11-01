// A problem report from the browser: the backend writes the document (the guide's settings and
// steps), the page posts it to the contribution server through the site's own /v1 relay.

import { invoke } from "@tauri-apps/api/core";

const ID_KEY = "factoruide.contributor";

/** This browser's report id: 32 random hex digits, made once. */
function contributorId(): string {
  const saved = localStorage.getItem(ID_KEY);
  if (saved && /^[0-9a-f]{32}$/.test(saved)) return saved;
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  const id = Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
  localStorage.setItem(ID_KEY, id);
  return id;
}

function system(): string {
  const p = navigator.userAgent;
  if (/Windows/i.test(p)) return "windows";
  if (/Mac/i.test(p)) return "macos";
  return "linux";
}

/** Sends a report (rejects with "rate_limited" or the reason). */
export async function sendWebIssue(draft: unknown): Promise<"accepted" | "already_received"> {
  const document = await invoke<unknown>("issue_document", { draft, contributor: contributorId(), os: system() });
  const r = await fetch("/v1/issues", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(document),
  });
  if (r.status === 201) return "accepted";
  if (r.status === 200) return "already_received";
  if (r.status === 429) throw "rate_limited";
  const body = (await r.json().catch(() => ({}))) as { error?: string; status?: string };
  throw `${r.status}: ${body.error ?? body.status ?? ""}`;
}
