import { useRef, useState } from "react";
import { Dialog } from "./Dialog";
import { normalizeAppError } from "../lib/api";
import { AppIcon } from "./icons";

export function ConfirmDialog({
  title,
  description,
  detail,
  confirmLabel = "Delete",
  busyLabel = "Deleting",
  tone = "danger",
  onClose,
  onConfirm
}: {
  title: string;
  description: string;
  detail?: string;
  confirmLabel?: string;
  busyLabel?: string;
  tone?: "danger" | "neutral";
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
      setError(normalizeAppError(reason).message);
      setBusy(false);
      inFlight.current = false;
    }
  }

  return (
    <Dialog title={title} description={description} onClose={busy ? () => undefined : onClose}>
      <div className={`confirm-body${tone === "neutral" ? " neutral" : ""}`}>
        <AppIcon name={tone === "neutral" ? "upload" : "warning"} size={22} />
        {detail ? <code>{detail}</code> : null}
        {error ? <div className="inline-error">{error}</div> : null}
      </div>
      <footer className="dialog-actions">
        <button className="secondary-button" onClick={onClose} disabled={busy}>Cancel</button>
        <button className={tone === "neutral" ? "primary-button" : "danger-button"} onClick={confirm} disabled={busy}>{tone === "neutral" ? null : <AppIcon name="trash" size={16} />}{busy ? busyLabel : confirmLabel}</button>
      </footer>
    </Dialog>
  );
}
