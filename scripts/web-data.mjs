// The static data of the web app (scripts/build-web.sh): each edition's database and map images
// as they are, its minimap pack split into tiles with their index, and the headers a host
// should send.
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const out = process.argv[2];
const data = fileURLToPath(new URL("../data/", import.meta.url));
for (const edition of ["forever", "classic", "tbc"]) {
  const db = join(data, `${edition}.sqlite.zst`);
  if (!existsSync(db)) continue;
  mkdirSync(join(out, "data"), { recursive: true });
  copyFileSync(db, join(out, "data", `${edition}.sqlite.zst`));
  const maps = join(data, "maps", edition);
  if (!existsSync(maps)) continue;
  mkdirSync(join(out, "data/maps", edition), { recursive: true });
  for (const f of readdirSync(maps).filter((f) => f.endsWith(".webp"))) copyFileSync(join(maps, f), join(out, "data/maps", edition, f));
  // FGMM, the index's length (u32 LE), the index (JSON [map, level, x, y, offset, length]), the images.
  const pack = join(maps, "minimap.pack");
  if (!existsSync(pack)) continue;
  const bytes = readFileSync(pack);
  if (bytes.subarray(0, 4).toString() !== "FGMM") throw new Error(`${pack}: not a minimap pack`);
  const len = bytes.readUInt32LE(4);
  const index = JSON.parse(bytes.subarray(8, 8 + len).toString());
  const images = bytes.subarray(8 + len);
  const dir = join(out, "data/minimap", edition);
  for (const [map, level, x, y, offset, length] of index) {
    mkdirSync(join(dir, String(map), String(level)), { recursive: true });
    writeFileSync(join(dir, String(map), String(level), `${x}_${y}.webp`), images.subarray(offset, offset + length));
  }
  writeFileSync(join(dir, "index.json"), JSON.stringify(index.map(([m, l, x, y]) => [m, l, x, y])));
}
// For hosts reading a _headers file (Netlify, Cloudflare Pages): the isolation the backend's
// threads need. Elsewhere coi.js gets it from a service worker.
writeFileSync(
  join(out, "_headers"),
  "/*\n  Cross-Origin-Opener-Policy: same-origin\n  Cross-Origin-Embedder-Policy: require-corp\n  Cross-Origin-Resource-Policy: same-origin\n",
);
