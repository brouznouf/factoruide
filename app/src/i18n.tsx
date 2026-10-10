import { createContext, useCallback, useContext, type ReactNode } from "react";
import { CATALOG } from "./catalog";

/** Languages of the app (the same as the guide's). */
export type Lang = "fr" | "en" | "de" | "es" | "esMX" | "pt" | "ru" | "ko" | "zhCN" | "zhTW";

export const LANGS: { key: Lang; label: string; locale: string }[] = [
  { key: "fr", label: "Français", locale: "frFR" },
  { key: "en", label: "English", locale: "enUS" },
  { key: "de", label: "Deutsch", locale: "deDE" },
  { key: "es", label: "Español (España)", locale: "esES" },
  { key: "esMX", label: "Español (México)", locale: "esMX" },
  { key: "pt", label: "Português (Brasil)", locale: "ptBR" },
  { key: "ru", label: "Русский", locale: "ruRU" },
  { key: "ko", label: "한국어", locale: "koKR" },
  { key: "zhCN", label: "简体中文", locale: "zhCN" },
  { key: "zhTW", label: "繁體中文", locale: "zhTW" },
];

/** Game locale (guide language) of an app language. */
export const gameLocale = (lang: Lang) => LANGS.find((l) => l.key === lang)?.locale ?? "enUS";

/** App language from the system language. */
export function detectLang(): Lang {
  const nav = (navigator.language || "en").toLowerCase();
  if (nav.startsWith("es-mx") || nav.startsWith("es-419")) return "esMX";
  if (nav.startsWith("zh")) return nav.includes("tw") || nav.includes("hk") || nav.includes("hant") ? "zhTW" : "zhCN";
  const short = nav.slice(0, 2);
  return (["fr", "en", "de", "es", "pt", "ru", "ko"] as const).find((l) => l === short) ?? "en";
}

const fill = (text: string, vars?: Record<string, string | number>) =>
  vars ? text.replace(/\{(\w+)\}/g, (m, k) => (k in vars ? String(vars[k]) : m)) : text;

/**
 * Text in `lang`. The French text is the key (the app was written in French); missing
 * translations fall back to English, then to French.
 */
export function translate(lang: Lang, fr: string, vars?: Record<string, string | number>): string {
  if (lang === "fr") return fill(fr, vars);
  const entry = CATALOG[fr];
  const text = entry?.[lang] ?? (lang === "esMX" ? entry?.es : undefined) ?? entry?.en ?? fr;
  return fill(text, vars);
}

const LangContext = createContext<Lang>("fr");

export function I18nProvider({ lang, children }: { lang: Lang; children: ReactNode }) {
  return <LangContext.Provider value={lang}>{children}</LangContext.Provider>;
}

export const useLang = () => useContext(LangContext);

/** `t("Texte français {n}", { n })` in the app language. */
export function useT() {
  const lang = useLang();
  return useCallback((fr: string, vars?: Record<string, string | number>) => translate(lang, fr, vars), [lang]);
}

/** BCP 47 tag of an app language (dates, numbers). */
export const bcp47 = (lang: Lang) =>
  ({ fr: "fr-FR", en: "en-US", de: "de-DE", es: "es-ES", esMX: "es-MX", pt: "pt-BR", ru: "ru-RU", ko: "ko-KR", zhCN: "zh-CN", zhTW: "zh-TW" })[lang];

/** Short date and time of unix seconds, in the app language. */
export function useDate() {
  const lang = useLang();
  return useCallback(
    (secs: number) => new Date(secs * 1000).toLocaleString(bcp47(lang), { dateStyle: "short", timeStyle: "short" }),
    [lang],
  );
}

// Race, class and faction names (the game data gives them in English).
// Order: fr, de, es, pt, ru, ko, zhCN, zhTW.
const GAME: Record<string, string[]> = {
  human: ["Humain", "Mensch", "Humano", "Humano", "Человек", "인간", "人类", "人類"],
  dwarf: ["Nain", "Zwerg", "Enano", "Anão", "Дворф", "드워프", "矮人", "矮人"],
  nightelf: ["Elfe de la nuit", "Nachtelf", "Elfo de la noche", "Elfo Noturno", "Ночной эльф", "나이트 엘프", "暗夜精灵", "夜精靈"],
  gnome: ["Gnome", "Gnom", "Gnomo", "Gnomo", "Гном", "노움", "侏儒", "地精"],
  orc: ["Orc", "Orc", "Orco", "Orc", "Орк", "오크", "兽人", "獸人"],
  undead: ["Mort-vivant", "Untoter", "No-muerto", "Morto-vivo", "Нежить", "언데드", "亡灵", "不死族"],
  scourge: ["Mort-vivant", "Untoter", "No-muerto", "Morto-vivo", "Нежить", "언데드", "亡灵", "不死族"],
  tauren: ["Tauren", "Tauren", "Tauren", "Tauren", "Таурен", "타우렌", "牛头人", "牛頭人"],
  troll: ["Troll", "Troll", "Trol", "Troll", "Тролль", "트롤", "巨魔", "食人妖"],
  warrior: ["Guerrier", "Krieger", "Guerrero", "Guerreiro", "Воин", "전사", "战士", "戰士"],
  paladin: ["Paladin", "Paladin", "Paladín", "Paladino", "Паладин", "성기사", "圣骑士", "聖騎士"],
  hunter: ["Chasseur", "Jäger", "Cazador", "Caçador", "Охотник", "사냥꾼", "猎人", "獵人"],
  rogue: ["Voleur", "Schurke", "Pícaro", "Ladino", "Разбойник", "도적", "潜行者", "盜賊"],
  priest: ["Prêtre", "Priester", "Sacerdote", "Sacerdote", "Жрец", "사제", "牧师", "牧師"],
  shaman: ["Chaman", "Schamane", "Chamán", "Xamã", "Шаман", "주술사", "萨满祭司", "薩滿"],
  mage: ["Mage", "Magier", "Mago", "Mago", "Маг", "마법사", "法师", "法師"],
  warlock: ["Démoniste", "Hexenmeister", "Brujo", "Bruxo", "Чернокнижник", "흑마법사", "术士", "術士"],
  druid: ["Druide", "Druide", "Druida", "Druida", "Друид", "드루이드", "德鲁伊", "德魯伊"],
  alliance: ["Alliance", "Allianz", "Alianza", "Aliança", "Альянс", "얼라이언스", "联盟", "聯盟"],
  horde: ["Horde", "Horde", "Horda", "Horda", "Орда", "호드", "部落", "部落"],
};
const GAME_LANGS: Lang[] = ["fr", "de", "es", "pt", "ru", "ko", "zhCN", "zhTW"];

/** Zone or dungeon name in the game language of the app language (English when unknown). */
export function useAreaName() {
  const lang = useLang();
  return useCallback((area: { name: string; names?: Record<string, string> }) => area.names?.[gameLocale(lang)] ?? area.name, [lang]);
}

/** Race, class or faction name in the app language (English name in, as is when unknown). */
export function useGameName() {
  const lang = useLang();
  return useCallback(
    (name: string) => {
      const row = GAME[name.toLowerCase().replace(/[^a-z]/g, "")];
      const i = GAME_LANGS.indexOf(lang === "esMX" ? "es" : lang);
      return row && i >= 0 ? row[i] : name;
    },
    [lang],
  );
}
