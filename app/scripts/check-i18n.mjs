// Fails when a text passed to t("...") has no entry in src/catalog.ts, or an entry has no English
// text (the fallback of every language): the app would show French there. Run by `npm run check:i18n` (CI: frontend job).
import fs from "node:fs";
import path from "node:path";

const root = path.join(import.meta.dirname, "..", "src");
// Other languages fall back to English: some advanced settings are only translated in English.
const REQUIRED = ["en"];

const catalog = fs.readFileSync(path.join(root, "catalog.ts"), "utf8");
const entries = new Map();
// The entries are flat, but a long one is written over several lines, and their
// translations hold {placeholder}s of their own.
for (const m of catalog.matchAll(/("(?:[^"\\]|\\.)*")\s*:\s*\{((?:[^{}]|\{[^{}]*\})*)\}/g)) {
  entries.set(JSON.parse(m[1]), m[2]);
}

const files = [];
(function walk(dir) {
  for (const f of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, f.name);
    if (f.isDirectory()) walk(p);
    else if (/\.tsx?$/.test(f.name) && f.name !== "catalog.ts") files.push(p);
  }
})(root);

const problems = [];
for (const file of files) {
  const lines = fs.readFileSync(file, "utf8").split("\n");
  lines.forEach((line, i) => {
    if (/^\s*(\/\/|\/\*|\*)/.test(line)) return;
    for (const m of line.matchAll(/\bt\(\s*("(?:[^"\\]|\\.)*")/g)) {
      const key = JSON.parse(m[1]);
      if (!entries.has(key)) {
        problems.push(`${path.relative(root, file)}:${i + 1}: no translation for ${JSON.stringify(key.slice(0, 80))}`);
      }
    }
  });
}

// Texts t() takes from a variable, unseen by the scan above: the settings' labels, helps, titles
// and option labels (paramSchema.ts, ConfigForm.tsx renders them through t()), the step verbs
// (guideTimeline.ts, the step tip shows them through t()), the equipment slots (BisPanel.tsx)
// and the items' slots and stats (ItemTip.tsx, named by the data's codes).
for (const [file, pattern] of [
  ["paramSchema.ts", /\b(?:label|help|title|verb)\s*:\s*("(?:[^"\\]|\\.)*")/g],
  ["components/guideTimeline.ts", /\bverb\s*:\s*("(?:[^"\\]|\\.)*")/g],
  ["components/ConfigForm.tsx", /\b(?:label|help)\s*:\s*("(?:[^"\\]|\\.)*")/g],
  ["components/BisPanel.tsx", /^\s*\d+:\s*("(?:[^"\\]|\\.)*")/gm],
  ["components/ItemTip.tsx", /^\s*(?:"(?:[^"\\]|\\.)*"|\d+|\w+)\s*:\s*("(?:[^"\\]|\\.)*")/gm],
]) {
  const p = path.join(root, file);
  const text = fs.readFileSync(p, "utf8");
  for (const m of text.matchAll(pattern)) {
    const key = JSON.parse(m[1]);
    if (/^[\d\W]*$/.test(key)) continue; // option values, icons: not texts
    if (!entries.has(key)) {
      problems.push(`${file}: no translation for ${JSON.stringify(key.slice(0, 80))}`);
    }
  }
}
for (const [key, body] of entries) {
  const absent = REQUIRED.filter((l) => !new RegExp(`\\b${l}\\s*:`).test(body));
  if (absent.length) problems.push(`catalog.ts: ${JSON.stringify(key.slice(0, 80))} misses ${absent.join(", ")}`);
}

if (problems.length) {
  console.error(problems.join("\n"));
  console.error(`\n${problems.length} translation problem(s): add the texts to src/catalog.ts`);
  process.exit(1);
}
console.log(`translations ok: ${entries.size} texts`);
