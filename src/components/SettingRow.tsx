import type { ReactNode } from "react";

/** A settings category: heading plus plain rows separated by dividers (no card chrome). */
export function SettingsSection({ id, title, children }: { id: string; title: string; children: ReactNode }) {
  return (
    <section className="set-section" id={`settings-${id}`} data-section={id} aria-labelledby={`settings-${id}-title`}>
      <h2 id={`settings-${id}-title`}>{title}</h2>
      {children}
    </section>
  );
}

export function SettingsGroup({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="set-group" role="group" aria-label={title}>
      <h3>{title}</h3>
      {children}
    </div>
  );
}

/** Label and description on the left, control on the right. `stack` puts wide controls under the text. */
export function SettingRow({ label, help, htmlFor, stack, children }: { label: string; help?: ReactNode; htmlFor?: string; stack?: boolean; children?: ReactNode }) {
  const labelId = htmlFor ? undefined : `row-${label.replace(/\W+/g, "-").toLowerCase()}`;
  return (
    <div className={stack ? "set-row stack" : "set-row"} role={htmlFor ? undefined : "group"} aria-labelledby={labelId}>
      <div className="set-text">
        {htmlFor ? <label htmlFor={htmlFor}>{label}</label> : <span id={labelId} className="set-label">{label}</span>}
        {help ? <p>{help}</p> : null}
      </div>
      {children ? <div className="set-control">{children}</div> : null}
    </div>
  );
}

/** Two-to-three way choice shown as one connected control. */
export function Segmented<T extends string>({ name, value, options, onChange }: { name: string; value: T; options: { value: T; label: string }[]; onChange: (value: T) => void }) {
  return (
    <div className="segmented" role="radiogroup">
      {options.map((option) => (
        <label key={option.value} className={option.value === value ? "on" : undefined}>
          <input type="radio" name={name} value={option.value} checked={option.value === value} onChange={() => onChange(option.value)} />
          {option.label}
        </label>
      ))}
    </div>
  );
}
