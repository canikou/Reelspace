import { AlertTriangle, Trash2 } from "lucide-react";
import { useRef, useState } from "react";
import { Dialog } from "./Dialog";

export function ConfirmDialog({
  title,
  description,
  detail,
  onClose,
  onConfirm
}: {
  title: string;
  description: string;
  detail?: string;
  onClose: () => void;
  onConfirm: () => Promise<void>;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inFlight = useRef(false);

  async function confirm() {
    if (inFlight.current) return;
    inFlight.current = true;
    setBusy(true);
    setError(null);
    try {
      await onConfirm();
    } catch (reason) {
      setError(String(reason));
      setBusy(false);
      inFlight.current = false;
    }
  }

  return (
    <Dialog title={title} description={description} onClose={busy ? () => undefined : onClose}>
      <div className="confirm-body">
        <AlertTriangle size={22} />
        {detail ? <code>{detail}</code> : null}
        {error ? <div className="inline-error">{error}</div> : null}
      </div>
      <footer className="dialog-actions">
        <button className="secondary-button" onClick={onClose} disabled={busy}>Cancel</button>
        <button className="danger-button" onClick={confirm} disabled={busy}><Trash2 size={16} /> {busy ? "Deleting" : "Delete"}</button>
      </footer>
    </Dialog>
  );
}
