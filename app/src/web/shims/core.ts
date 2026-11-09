// `@tauri-apps/api/core` in the web build: the calls go to the backend in Web Workers
// (host.ts), but the maps' images and the items' icons, which are static files of the site.
import { getBackend } from "../host";
import { currentEdition, itemIcon, mapImage, minimapImage, minimapIndex } from "../statics";

export async function invoke<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  switch (cmd) {
    case "map_image":
      return (await mapImage(await currentEdition(), args.id as number)) as T;
    case "minimap_index":
      return (await minimapIndex(await currentEdition())) as T;
    case "minimap_image":
      return (await minimapImage(await currentEdition(), args.map as number, args.level as number, args.x as number, args.y as number)) as T;
    case "item_icon":
      return (await itemIcon(await currentEdition(), args.item as number)) as T;
    case "open_link":
      window.open(String(args.url), "_blank", "noopener");
      return undefined as T;
  }
  return getBackend().call<T>(cmd, args);
}
