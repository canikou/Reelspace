import { type ReactNode, useEffect, useId, useRef } from "react";
import { AppIcon } from "./icons";

export function Dialog({
  title,
  description,
  onClose,
  children,
  size = "compact"
}: {
  title: string;
  description?: string;
  onClose: () => void;
  children: ReactNode;
  size?: "compact" | "wide";
}) {
  const titleId = useId();
  const descriptionId = useId();
  const panelRef = useRef<HTMLElement | null>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    // Land on the field, else the safe footer action (Cancel), never the header close button.
    const panel = panelRef.current;
    (panel?.querySelector<HTMLElement>("input, textarea, select") ?? panel?.querySelector<HTMLElement>(".dialog-actions button, .collection-picker button") ?? panel?.querySelector<HTMLElement>("button"))?.focus();
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") onCloseRef.current();
    }
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      previous?.focus();
    };
  }, []);

  return (
    <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <section
        ref={panelRef}
        className={`dialog-panel ${size}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={description ? descriptionId : undefined}
      >
        <header className="modal-header">
          <div>
            <h2 id={titleId}>{title}</h2>
            {description ? <p id={descriptionId}>{description}</p> : null}
          </div>
          <button className="bare-icon" aria-label="Close" title="Close" onClick={onClose}>
            <AppIcon name="close" size={18} />
          </button>
        </header>
        {children}
      </section>
    </div>
  );
}
