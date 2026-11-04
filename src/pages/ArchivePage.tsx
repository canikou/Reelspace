import { formatBytes } from "../lib/format";
import type { UploadJob } from "../lib/types";
import { AppIcon, AvailabilityIcon, type AvailabilityState } from "../components/icons";

const stateIcon = (state: string): AvailabilityState => ({
  queued: "queued", retry: "queued", paused: "queued", preparing: "preparing", uploading: "uploading",
  waiting_for_quota: "waitingForQuota", processing_remote: "processingRemote", verifying: "preparing",
  archived: "archivedVerified", failed: "uploadFailed", remote_missing: "remoteMissing"
}[state] as AvailabilityState ?? "queued");

export function ArchivePage({ jobs, onControl }: { jobs: UploadJob[]; onControl: (action: string, jobId?: string) => Promise<void> }) {
  const active = jobs.filter((job) => !["archived", "failed"].includes(job.state));
  return (
    <div className="page-stack">
      <div className="section-title"><div><h2>Archive queue</h2><span>{active.length} active</span></div><div className="section-actions"><button onClick={() => onControl("pauseAll")}><AppIcon name="cancel" size={16} /> Pause all</button><button onClick={() => onControl("resumeAll")}><AppIcon name="upload" size={16} /> Resume</button></div></div>
      <section className="archive-summary">
        <article><AppIcon name="archive" size={20} /><div><strong>Private YouTube archive</strong><span>Verified preservation, not cloud sync</span></div></article>
        <article><AppIcon name="protected" size={20} /><div><strong>Keep Local stays independent</strong><span>Archived files are never cleaned automatically while protected</span></div></article>
      </section>
      <section className="queue-list">
        {jobs.map((job) => <article className="queue-row" key={job.id}>
          <AvailabilityIcon state={stateIcon(job.state)} size={18} />
          <div><strong>{job.filename}</strong><span>{job.error ?? `${formatBytes(job.bytesUploaded)} / ${formatBytes(job.totalBytes)}`}</span></div>
          <div className="queue-progress"><span style={{ width: `${job.totalBytes ? Math.min(100, job.bytesUploaded / job.totalBytes * 100) : 0}%` }} /></div>
          <div className="queue-actions">
            {job.state === "failed" ? <button onClick={() => onControl("retry", job.id)}>Retry</button> : null}
            {["queued", "retry", "waiting_for_quota"].includes(job.state) ? <button onClick={() => onControl("pause", job.id)}>Pause</button> : null}
            {job.state === "paused" ? <button onClick={() => onControl("resume", job.id)}>Resume</button> : null}
            {!['archived', 'uploading'].includes(job.state) ? <button className="destructive-link" onClick={() => onControl("cancel", job.id)}>Cancel</button> : null}
          </div>
        </article>)}
        {!jobs.length ? <div className="empty-state"><AppIcon name="archive" size={26} />Nothing is queued. Archive actions are available from recording and clip cards.</div> : null}
      </section>
    </div>
  );
}
