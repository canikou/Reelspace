import { AppIcon, FrameStackMark } from "./icons";

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
    <div className="modal-backdrop" onKeyDown={(event) => event.key === "Escape" && onDismiss()}>
      <section className="setup-modal" role="dialog" aria-modal="true" aria-labelledby="setup-title">
        <header className="modal-header">
          <div>
            <h2 id="setup-title">Choose a capture folder</h2>
            <p>Reelspace needs one source folder before it can scan for raw videos.</p>
          </div>
          <button title="Dismiss" aria-label="Dismiss" onClick={onDismiss}><AppIcon name="close" size={18} /></button>
        </header>
        <div className="setup-body">
          <div className="setup-mark" aria-hidden="true"><FrameStackMark size={38} /></div>
          <button autoFocus className="primary-button" onClick={onChoose} disabled={busy} title="Choose source folder">
            <AppIcon name="add" size={18} />
            {busy ? "Opening..." : "Choose Folder"}
          </button>
        </div>
      </section>
    </div>
  );
}
