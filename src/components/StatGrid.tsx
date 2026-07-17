import { HardDrive, Scissors, Video, WalletCards } from "lucide-react";
import { formatBytes, percent } from "../lib/format";
import type { StorageStats } from "../lib/types";

export function StatGrid({ stats, rawCount, clipCount }: { stats: StorageStats; rawCount: number; clipCount: number }) {
  const cards = [
    { label: "Drive Free", value: formatBytes(stats.driveAvailableBytes), detail: "available", icon: HardDrive, tone: "info" },
    { label: "Raw Files", value: formatBytes(stats.rawBytes), detail: `${rawCount} files`, icon: Video, tone: "raw" },
    { label: "Clips", value: formatBytes(stats.clipBytes), detail: `${clipCount} clips`, icon: Scissors, tone: "clip" },
    { label: "Managed", value: formatBytes(stats.managedBytes), detail: `${rawCount + clipCount} total`, icon: WalletCards, tone: "accent" }
  ];

  return (
    <section className="stat-grid">
      {cards.map((card) => {
        const Icon = card.icon;
        return (
          <article className={`stat-card ${card.tone}`} key={card.label}>
            <div className="stat-icon"><Icon size={19} /></div>
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
