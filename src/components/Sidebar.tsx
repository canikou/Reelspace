import { AppIcon, FrameStackMark, type IconName } from "./icons";

export type PageKey = "home" | "gallery" | "clips" | "collections" | "settings";

const items: Array<{ key: PageKey; label: string; icon: IconName }> = [
  { key: "home", label: "Home", icon: "home" },
  { key: "gallery", label: "Recordings", icon: "recording" },
  { key: "clips", label: "Clips", icon: "clip" },
  { key: "collections", label: "Collections", icon: "collection" },
  { key: "settings", label: "Settings", icon: "settings" }
];

export function Sidebar({ page, onPageChange }: { page: PageKey; onPageChange: (page: PageKey) => void }) {
  return (
    <aside className="sidebar">
      <div className="brand" title="Reelspace">
        <FrameStackMark size={28} />
      </div>
      <nav className="nav-list" aria-label="Main navigation">
        {items.map((item) => {
          return (
            <button
              key={item.key}
              className={page === item.key ? "nav-button active" : "nav-button"}
              onClick={() => onPageChange(item.key)}
              title={item.label}
              aria-label={item.label}
              aria-current={page === item.key ? "page" : undefined}
            >
              <AppIcon name={item.icon} size={21} />
              <span>{item.label}</span>
            </button>
          );
        })}
      </nav>
    </aside>
  );
}
