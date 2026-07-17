import { FolderPlus, X } from "lucide-react";

export function SetupPrompt({
  busy,
  onChoose,
  onDismiss
}: {
  busy: boolean;
  onChoose: () => void;
  onDismiss: () => void;
}) {
  return (
    <div className="modal-backdrop">
      <section className="setup-modal" role="dialog" aria-modal="true" aria-labelledby="setup-title">
        <header className="modal-header">
          <div>
            <h2 id="setup-title">Choose a capture folder</h2>
            <p>Reelspace needs one source folder before it can scan for raw videos.</p>
          </div>
          <button title="Dismiss" aria-label="Dismiss" onClick={onDismiss}><X size={18} /></button>
        </header>
        <div className="setup-body">
          <button className="primary-button" onClick={onChoose} disabled={busy} title="Choose source folder">
            <FolderPlus size={18} />
            {busy ? "Opening..." : "Choose Folder"}
          </button>
        </div>
      </section>
    </div>
  );
}
