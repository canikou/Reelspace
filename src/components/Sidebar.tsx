import { Clapperboard, FolderOpen, Home, Library, Settings, Tags, type LucideIcon } from "lucide-react";

export type PageKey = "home" | "gallery" | "clips" | "collections" | "settings";

const items: Array<{ key: PageKey; label: string; icon: LucideIcon }> = [
  { key: "home", label: "Home", icon: Home },
  { key: "gallery", label: "Gallery", icon: FolderOpen },
  { key: "clips", label: "Clips", icon: Clapperboard },
  { key: "collections", label: "Collections", icon: Library },
  { key: "settings", label: "Settings", icon: Settings }
];

export function Sidebar({ page, onPageChange }: { page: PageKey; onPageChange: (page: PageKey) => void }) {
  return (
    <aside className="sidebar">
      <div className="brand" title="Reelspace">
        <Tags size={22} />
      </div>
      <nav className="nav-list" aria-label="Main navigation">
        {items.map((item) => {
          const Icon = item.icon;
          return (
            <button
              key={item.key}
              className={page === item.key ? "nav-button active" : "nav-button"}
              onClick={() => onPageChange(item.key)}
              title={item.label}
              aria-label={item.label}
            >
              <Icon size={22} />
            </button>
          );
        })}
      </nav>
    </aside>
  );
}
