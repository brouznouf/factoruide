import type { Params } from "../api";
import type { Field } from "../paramSchema";
import { useT } from "../i18n";

interface Props {
  field: Field;
  params: Params;
  defaults: Params;
  onChange: (key: keyof Params, value: Params[keyof Params]) => void;
}

/** One setting, shown in its display unit (e.g. minutes for a value stored in seconds). */
export function ParamField({ field, params, defaults, onChange }: Props) {
  const t = useT();
  const value = params[field.key];
  const changed = JSON.stringify(value) !== JSON.stringify(defaults[field.key]);
  const reset = changed ? (
    <button className="link" title={t("Valeur par défaut")} onClick={() => onChange(field.key, defaults[field.key])}>
      ↺
    </button>
  ) : null;

  if (field.kind === "bool") {
    return (
      <label className="field field-bool" title={field.help ? t(field.help) : undefined}>
        <input type="checkbox" checked={Boolean(value)} onChange={(e) => onChange(field.key, e.target.checked)} />
        <span>{t(field.label)}</span>
        {reset}
        {field.help && <small>{t(field.help)}</small>}
      </label>
    );
  }

  if (field.kind === "select") {
    return (
      <label className="field">
        <span>
          {t(field.label)} {reset}
        </span>
        <select value={String(value)} onChange={(e) => onChange(field.key, e.target.value)}>
          {field.options.map((o) => (
            <option key={o.value} value={o.value}>
              {t(o.label)}
            </option>
          ))}
        </select>
        {field.help && <small>{t(field.help)}</small>}
      </label>
    );
  }

  const scale = field.scale ?? 1;
  return (
    <label className="field">
      <span>
        {t(field.label)} {reset}
      </span>
      <div className="field-number">
        <input
          type="number"
          value={Math.round(((value as number) / scale) * 1000) / 1000}
          min={field.min}
          max={field.max}
          step={field.step}
          onChange={(e) => {
            const v = Number(e.target.value);
            if (!Number.isNaN(v)) onChange(field.key, v * scale);
          }}
        />
        {field.unit && <em>{t(field.unit)}</em>}
      </div>
      {field.help && <small>{t(field.help)}</small>}
    </label>
  );
}
