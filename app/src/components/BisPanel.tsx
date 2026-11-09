import { useState } from "react";
import type { BisSlot } from "../api";
import { useT } from "../i18n";
import { ItemHover, QUALITY, useIcon, useItemInfo } from "./ItemTip";
import chest from "../assets/slots/chest.png";
import feet from "../assets/slots/feet.png";
import finger from "../assets/slots/finger.png";
import hands from "../assets/slots/hands.png";
import head from "../assets/slots/head.png";
import legs from "../assets/slots/legs.png";
import mainHand from "../assets/slots/main-hand.png";
import neck from "../assets/slots/neck.png";
import ranged from "../assets/slots/ranged.png";
import secondaryHand from "../assets/slots/secondary-hand.png";
import shirt from "../assets/slots/shirt.png";
import shoulder from "../assets/slots/shoulder.png";
import trinket from "../assets/slots/trinket.png";
import waist from "../assets/slots/waist.png";
import wrists from "../assets/slots/wrists.png";

/** The equipment slots of the power model (`fg_route::power`), by index: their name. */
const SLOTS: Record<number, string> = {
  0: "Tête",
  1: "Cou",
  2: "Épaules",
  3: "Dos",
  4: "Torse",
  5: "Poignets",
  6: "Mains",
  7: "Ceinture",
  8: "Jambes",
  9: "Pieds",
  10: "Doigt",
  12: "Bibi",
  14: "Main droite",
  15: "Main gauche",
  16: "Deux mains",
  17: "À distance",
};

/** The game's own empty-slot picture of each (read out of the client): the back has none of its
 * own (a garment's serves, the game's sheet having none either) nor the two-hand slot (a
 * two-handed weapon sits in the main hand). */
const PICTURES: Record<number, string> = {
  0: head,
  1: neck,
  2: shoulder,
  3: shirt,
  4: chest,
  5: wrists,
  6: hands,
  7: waist,
  8: legs,
  9: feet,
  10: finger,
  12: trinket,
  14: mainHand,
  15: secondaryHand,
  16: mainHand,
  17: ranged,
};

/** The slots as the character sheet lays them out: two columns beside the middle, the weapons below. */
const LEFT = [0, 1, 2, 3, 4, 5];
const RIGHT = [6, 7, 8, 9, 10, 12];
const WEAPONS = [14, 15, 16, 17];

/**
 * The best gear of the guide (`Route::bis`), one level band at a time, laid out as the game's
 * own character sheet: the game's empty-slot pictures, the planned items' icons over them with
 * their quality's border, their details on hover, and where each comes from in the middle.
 */
export function BisPanel({ bis }: { bis: BisSlot[] }) {
  const t = useT();
  const [band, setBand] = useState(bis[0]?.level ?? 0);
  const bands = new Map<number, BisSlot[]>();
  for (const entry of bis) {
    const list = bands.get(entry.level) ?? [];
    list.push(entry);
    bands.set(entry.level, list);
  }
  const entries = bands.get(band) ?? bands.get(bis[0]?.level ?? 0) ?? [];
  const bySlot = new Map(entries.map((entry) => [entry.slot, entry]));
  return (
    <div className="pd-paperdoll">
      <div className="view-tabs pd-bands" role="tablist">
        {[...bands.keys()].map((level) => (
          <button
            key={level}
            role="tab"
            aria-selected={level === band}
            className={level === band ? "active" : ""}
            onClick={() => setBand(level)}
          >
            {t("Niveaux {from} – {to}", { from: level - 4, to: level })}
          </button>
        ))}
      </div>
      <div className="pd-columns">
        <div className="pd-column">
          {LEFT.map((slot) => (
            <PdSlot key={`${band} ${slot}`} slot={slot} entry={bySlot.get(slot)} />
          ))}
        </div>
        <ul className="pd-list">
          {entries.map((entry) => (
            <PdRow key={`${band} ${entry.slot}`} entry={entry} />
          ))}
        </ul>
        <div className="pd-column">
          {RIGHT.map((slot) => (
            <PdSlot key={`${band} ${slot}`} slot={slot} entry={bySlot.get(slot)} />
          ))}
        </div>
      </div>
      <div className="pd-weapons">
        {WEAPONS.map((slot) => (
          <PdSlot key={`${band} ${slot}`} slot={slot} entry={bySlot.get(slot)} />
        ))}
      </div>
    </div>
  );
}

/** One slot of the sheet: the game's empty picture, the planned item's icon over it with its
 * quality's border (its details on hover), or the empty picture alone when the band plans none. */
function PdSlot({ slot, entry }: { slot: number; entry?: BisSlot }) {
  const t = useT();
  const item = entry?.item ?? 0;
  const info = useItemInfo(item);
  const icon = useIcon(item);
  const name = t(SLOTS[slot] ?? "?");
  const color = info ? (QUALITY[info.quality] ?? "#ffffff") : undefined;
  const place = (
    <span
      className={entry ? "pd-slot has" : "pd-slot"}
      style={entry && color ? { borderColor: color } : undefined}
      role="img"
      aria-label={entry ? `${name} : ${entry.name}` : name}
      title={entry ? undefined : name}
    >
      <img className="pd-empty" src={PICTURES[slot]} alt="" />
      {entry && icon && <img className="pd-icon" src={icon} alt="" />}
    </span>
  );
  if (!entry) return place;
  return (
    <ItemHover item={item} name={entry.name}>
      {place}
    </ItemHover>
  );
}

/** One planned item of the band, in the middle of the sheet: its icon and name in its quality's
 * color (its details on hover), and where it comes from. */
function PdRow({ entry }: { entry: BisSlot }) {
  const t = useT();
  const info = useItemInfo(entry.item);
  const icon = useIcon(entry.item);
  return (
    <li>
      <ItemHover item={entry.item} name={entry.name}>
        {icon && <img className="pd-list-icon" src={icon} alt="" width={20} height={20} />}
        <strong className="pd-list-name" style={{ color: info ? (QUALITY[info.quality] ?? "#ffffff") : undefined }}>
          {entry.name}
        </strong>
      </ItemHover>
      <small>
        {entry.source.kind === "quest"
          ? t("Quête : {name}", { name: entry.source.quest_name })
          : t("Marchands : {names}", { names: entry.source.npcs.map((v) => v.name).join(", ") })}
      </small>
    </li>
  );
}
