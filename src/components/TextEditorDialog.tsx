import { useRef, useState } from "react";
import { Dialog } from "./Dialog";
import { AppIcon } from "./icons";

export function TextEditorDialog({
  title,
  description,
  initialValue = "",
  multiline = false,
  placeholder,
  onClose,
  onSave
}: {
  title: string;
  description?: string;
  initialValue?: string;
  multiline?: boolean;
  placeholder?: string;
  onClose: () => void;
  onSave: (value: string) => Promise<void>;
}) {
  const [value, setValue] = useState(initialValue);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inFlight = useRef(false);

  async function save() {
    if (inFlight.current) return;
    inFlight.current = true;
    setBusy(true);
    setError(null);
    try {
      await onSave(value);
    } catch (reason) {
      setError(String(reason));
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  }

  return (
    <Dialog title={title} description={description} onClose={busy ? () => undefined : onClose}>
      <div className="dialog-body">
        {multiline ? (
          <textarea
            aria-label={title}
            value={value}
            onChange={(event) => setValue(event.target.value)}
            placeholder={placeholder}
            rows={6}
          />
        ) : (
          <input
            aria-label={title}
            value={value}
            onChange={(event) => setValue(event.target.value)}
            placeholder={placeholder}
            onKeyDown={(event) => event.key === "Enter" && save()}
          />
        )}
        {error ? <div className="inline-error">{error}</div> : null}
      </div>
      <footer className="dialog-actions">
        <button className="secondary-button" onClick={onClose} disabled={busy}>Cancel</button>
        <button className="primary-button" onClick={save} disabled={busy}>
          <AppIcon name="save" size={16} /> {busy ? "Saving" : "Save"}
        </button>
      </footer>
    </Dialog>
  );
}
