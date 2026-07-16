import { HardDrive, Scissors, Video, WalletCards } from "lucide-react";
import { formatBytes, percent } from "../lib/format";
import type { StorageStats } from "../lib/types";

export function StatGrid({ stats }: { stats: StorageStats }) {
  const cards = [
    { label: "Drive Free", value: formatBytes(stats.driveAvailableBytes), icon: HardDrive },
    { label: "Raw Files", value: formatBytes(stats.rawBytes), icon: Video },
    { label: "Clips", value: formatBytes(stats.clipBytes), icon: Scissors },
    { label: "Managed", value: formatBytes(stats.managedBytes), icon: WalletCards }
  ];

  return (
    <section className="stat-grid">
      {cards.map((card) => {
        const Icon = card.icon;
        return (
          <article className="stat-card" key={card.label}>
            <div className="stat-icon"><Icon size={20} /></div>
            <span>{card.label}</span>
            <strong>{card.value}</strong>
          </article>
        );
      })}
      <article className="limit-card">
        <div className="limit-row">
          <span>Storage Limit</span>
          <strong>{stats.storageLimitBytes ? formatBytes(stats.storageLimitBytes) : "Not set"}</strong>
        </div>
        <div className="meter" aria-label="Storage limit usage">
          <div style={{ width: `${percent(stats.managedBytes, stats.storageLimitBytes)}%` }} />
        </div>
      </article>
    </section>
  );
}
