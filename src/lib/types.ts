export type MediaKind = "raw" | "clip";

export interface AppSettings {
  sourceFolders: string[];
  workingFolder?: string | null;
  clipFolder?: string | null;
  storageLimitBytes?: number | null;
  cleanupThresholdPercent: number;
}

export interface ToolHealth {
  ffmpegPath?: string | null;
  ffprobePath?: string | null;
  ready: boolean;
  message: string;
}

export interface StorageStats {
  driveTotalBytes: number;
  driveAvailableBytes: number;
  rawBytes: number;
  clipBytes: number;
  managedBytes: number;
  storageLimitBytes?: number | null;
  isNearLimit: boolean;
}

export interface AppSnapshot {
  settings: AppSettings;
  ffmpeg: ToolHealth;
  stats: StorageStats;
}

export interface MediaItem {
  id: string;
  kind: MediaKind;
  path: string;
  sourcePath?: string | null;
  filename: string;
  fileSize: number;
  durationSeconds: number;
  width?: number | null;
  height?: number | null;
  codec?: string | null;
  tags: string[];
  notes?: string | null;
  thumbnailPath?: string | null;
  createdAt: number;
  modifiedAt: number;
  trimStartSeconds?: number | null;
  trimEndSeconds?: number | null;
}

export interface Collection {
  id: string;
  name: string;
  itemCount: number;
}

export interface CreateClipRequest {
  sourceId: string;
  startSeconds: number;
  endSeconds: number;
  title?: string | null;
  enabledAudioTrackIndices: number[];
}

export interface AudioTrack {
  index: number;
  codec?: string | null;
  channels?: number | null;
  language?: string | null;
  title?: string | null;
  isDefault: boolean;
}
