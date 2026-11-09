// The static files of the site the pages read directly: the maps' images and the minimap's
// tiles, written by scripts/build-web.sh under data/.
import { getBackend } from "./host";

/** The game version the app works on. */
export const currentEdition = () => getBackend().call<{ edition: string }>("get_settings").then((s) => s.edition);

async function bytes(url: string): Promise<ArrayBuffer> {
  const r = await fetch(url);
  if (!r.ok) throw new Error(`${url}: ${r.status}`);
  return r.arrayBuffer();
}

export const mapImage = (edition: string, id: number) => bytes(`data/maps/${edition}/${id}.webp`);

/** `[map, level, x, y]` of each tile of an edition's minimap (none when it has no minimap). */
export async function minimapIndex(edition: string): Promise<[number, number, number, number][]> {
  const r = await fetch(`data/minimap/${edition}/index.json`);
  return r.ok ? r.json() : [];
}

export const minimapImage = (edition: string, map: number, level: number, x: number, y: number) =>
  bytes(`data/minimap/${edition}/${map}/${level}/${x}_${y}.webp`);

/** The icon packs of the editions, parsed once each: the index (each item at its picture) and
 * the pictures' bytes. */
const iconPacks = new Map<string, Promise<{ index: Map<number, [number, number]>; pictures: ArrayBuffer }>>();

/** The icon of an item (a PNG), out of the edition's pack `fg icons` writes (FGIC: the index's
 * length (u32 LE), the index, the pictures). */
export async function itemIcon(edition: string, item: number): Promise<ArrayBuffer> {
  let pack = iconPacks.get(edition);
  if (!pack) {
    pack = bytes(`data/icons/${edition}/icons.pack`).then((buffer) => {
      // A Uint8Array stringifies to its numbers joined: the bytes decode.
      const b = new Uint8Array(buffer);
      const text = (from: number, to: number) => new TextDecoder().decode(b.subarray(from, to));
      if (text(0, 4) !== "FGIC") {
        throw new Error(`data/icons/${edition}/icons.pack: not an icon pack`);
      }
      const len = new DataView(buffer).getUint32(4, true);
      const index: [number, number, number][] = JSON.parse(text(8, 8 + len));
      const pictures = buffer.slice(8 + len);
      return { index: new Map(index.map(([i, o, n]) => [i, [o, n]])), pictures };
    });
    iconPacks.set(edition, pack);
  }
  const { index, pictures } = await pack;
  const at = index.get(item);
  if (!at) throw new Error(`no icon of item ${item}`);
  return pictures.slice(at[0], at[0] + at[1]);
}
