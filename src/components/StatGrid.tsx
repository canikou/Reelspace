import { formatBytes, percent } from "../lib/format";
import type { StorageStats } from "../lib/types";
import { AppIcon, type IconName } from "./icons";

export function StatGrid({ stats, rawCount, clipCount }: { stats: StorageStats; rawCount: number; clipCount: number }) {
  const cards = [
    { label: "Drive Free", value: formatBytes(stats.driveAvailableBytes), detail: "available", icon: "local" as IconName, tone: "info" },
    { label: "Recordings", value: formatBytes(stats.rawBytes), detail: `${rawCount} files`, icon: "recording" as IconName, tone: "raw" },
    { label: "Clips", value: formatBytes(stats.clipBytes), detail: `${clipCount} clips`, icon: "clip" as IconName, tone: "clip" },
    { label: "Managed", value: formatBytes(stats.managedBytes), detail: `${rawCount + clipCount} total`, icon: "library" as IconName, tone: "accent" }
  ];

  return (
    <section className="stat-grid">
      {cards.map((card) => {
        return (
          <article className={`stat-card ${card.tone}`} key={card.label}>
            <div className="stat-icon"><AppIcon name={card.icon} size={19} /></div>
            <span>{card.label}</span>
            <strong>{card.value}</strong>
            <small>{card.detail}</small>
          </article>
        );
      })}
      <article className="limit-card">
        <div className="limit-row">
          <span>Storage Limit</span>
          <strong>{stats.storageLimitBytes ? formatBytes(stats.storageLimitBytes) : "Not set"}</strong>
        </div>
        {stats.storageLimitBytes ? (
          <div className={`meter ${stats.isNearLimit ? "warning" : ""}`} aria-label="Storage limit usage">
            <div style={{ width: `${percent(stats.managedBytes, stats.storageLimitBytes)}%` }} />
          </div>
        ) : <span className="muted">Set a limit in Settings</span>}
      </article>
    </section>
  );
}
