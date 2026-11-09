import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { SETTINGS_CHANGED, api, type ItemInfo } from "../api";
import { useT } from "../i18n";
import { Money } from "./Money";

/** An item's icon (an object URL), fetched once per mount: null without one (or without an item). */
export function useIcon(item: number): string | null {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    if (item <= 0) return;
    let alive = true;
    let made: string | null = null;
    api.itemIcon(item).then((fresh) => {
      if (!alive) {
        if (fresh) URL.revokeObjectURL(fresh);
        return;
      }
      made = fresh;
      setUrl(fresh);
    });
    return () => {
      alive = false;
      if (made) URL.revokeObjectURL(made);
    };
  }, [item]);
  return url;
}

/** The items read so far, by ID: each is read once, until the settings change (another game
 * version reads another database). */
const ITEMS = new Map<number, Promise<ItemInfo | null>>();

const read = (item: number) => {
  let known = ITEMS.get(item);
  if (!known) {
    known = api.itemInfo(item);
    ITEMS.set(item, known);
  }
  return known;
};

// Another game version: what was read of the last one is read again.
window.addEventListener(SETTINGS_CHANGED, () => ITEMS.clear());

/** An item's details (`null` until read, without an item, or when the game's data has none of it). */
export function useItemInfo(item: number): ItemInfo | null {
  const [info, setInfo] = useState<ItemInfo | null>(null);
  useEffect(() => {
    if (item <= 0) return;
    let alive = true;
    read(item).then((fresh) => {
      if (alive) setInfo(fresh);
    });
    return () => {
      alive = false;
    };
  }, [item]);
  return info;
}

/** Where an item goes (`InventoryType`), as its key of the catalog. */
const FITS: Record<number, string> = {
  1: "Tête",
  2: "Cou",
  3: "Épaules",
  4: "Haut",
  5: "Torse",
  6: "Ceinture",
  7: "Jambes",
  8: "Pieds",
  9: "Poignets",
  10: "Mains",
  11: "Doigt",
  12: "Bibi",
  13: "À une main",
  14: "Bouclier",
  15: "À distance",
  16: "Dos",
  17: "Deux mains",
  19: "Tabard",
  20: "Torse",
  21: "Main droite",
  22: "Main gauche",
  23: "Tenu en main gauche",
  25: "Arme de jet",
  26: "À distance",
  28: "Idole",
};

/** A stat's name, as the tooltip writes it, by the code the data gives. */
const STAT_LABELS: Record<string, string> = {
  str: "Force",
  agi: "Agilité",
  sta: "Endurance",
  int: "Intelligence",
  spi: "Esprit",
  ap: "Puissance d'attaque",
  rap: "Puissance d'attaque à distance",
  sp: "Puissance des sorts",
  heal: "Soins des sorts",
  crit: "Score de coup critique",
  spellCrit: "Score de coup critique des sorts",
  hit: "Score de toucher",
  mp5: "Mana toutes les 5 secondes",
};

/** Each quality's color, as the game writes them. */
export const QUALITY = ["#9d9d9d", "#ffffff", "#1eff00", "#0070dd", "#a335ee", "#ff8000", "#e6cc80"];

/**
 * An item's details as the game's tooltip shows them: its picture beside its name colored by its
 * quality, where it goes, what it gives, the level it asks and what it sells for. Nothing
 * without data of it: `info` and `icon` read by the caller, or by `item` here.
 */
export function ItemBlock({ item, name, info, icon }: { item: number; name?: string; info?: ItemInfo | null; icon?: string | null }) {
  const t = useT();
  const own = useItemInfo(item);
  const ownIcon = useIcon(item);
  const data = info ?? own;
  const picture = icon ?? ownIcon;
  if (!data) return null;
  const fit = FITS[data.inventoryType];
  const title = data.name || name;
  return (
    <div className="item-block">
      {(picture || title) && (
        <div className="item-head">
          {picture && <img className="item-icon" src={picture} alt="" width={36} height={36} />}
          {title && (
            <strong className="item-name" style={{ color: QUALITY[data.quality] ?? "#ffffff" }}>
              {title}
            </strong>
          )}
        </div>
      )}
      {fit && <div className="item-fit">{t(fit)}</div>}
      {data.armor > 0 && <div className="item-armor">{t("Armure : {n}", { n: data.armor })}</div>}
      {data.dps > 0 && (
        <div className="item-weapon">
          {t("{dps} dégâts par seconde (vitesse {speed})", { dps: data.dps.toFixed(1), speed: data.speed })}
        </div>
      )}
      {data.stats.map(([code, value]) => (
        <div key={code} className="item-stat">
          +{Math.round(value * 10) / 10} {t(STAT_LABELS[code] ?? code)}
        </div>
      ))}
      {data.requiredLevel > 1 && (
        <div className="item-required">{t("Niveau {n} requis", { n: data.requiredLevel })}</div>
      )}
      {data.sellPrice > 0 && (
        <div className="item-price">
          {t("Prix de vente :")} <Money copper={data.sellPrice} />
        </div>
      )}
    </div>
  );
}

/**
 * What the pointer holds the item's details over: hover (or focus) shows them in a bubble
 * beside, kept inside the window, once they are read (nothing while they load, nor without
 * data of the item — `name` stands in when the data has no name of it).
 */
export function ItemHover({ item, name, children }: { item: number; name?: string; children: ReactNode }) {
  const info = useItemInfo(item);
  const icon = useIcon(item);
  const ref = useRef<HTMLSpanElement>(null);
  const tip = useRef<HTMLDivElement>(null);
  const [at, setAt] = useState<DOMRect | null>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);

  // Right of what is hovered, else left of it; never past the window's edges. Measured again
  // when the details and the picture arrive: the bubble grows with them.
  useLayoutEffect(() => {
    if (!at || !tip.current || info == null) return;
    const { width, height } = tip.current.getBoundingClientRect();
    const left = at.right + 10 + width <= window.innerWidth - 8 ? at.right + 10 : Math.max(8, at.left - width - 10);
    setPos({ left, top: Math.max(8, Math.min(at.top - 6, window.innerHeight - height - 8)) });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- the icon changes what is measured
  }, [at, info, icon]);

  const show = () => ref.current && setAt(ref.current.getBoundingClientRect());
  const hide = () => {
    setAt(null);
    setPos(null);
  };
  return (
    <>
      <span ref={ref} className="item-hover" tabIndex={0} onMouseEnter={show} onMouseLeave={hide} onFocus={show} onBlur={hide}>
        {children}
      </span>
      {at &&
        info != null &&
        createPortal(
          <div ref={tip} className="item-tip" role="tooltip" style={pos ?? { left: 0, top: 0, visibility: "hidden" }}>
            <ItemBlock item={item} name={name} info={info} icon={icon} />
          </div>,
          document.body,
        )}
    </>
  );
}
