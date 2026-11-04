export type MediaKind = "raw" | "clip";

export type AppErrorCode =
  | "invalidRequest"
  | "invalidPath"
  | "pathOutsideManagedRoots"
  | "sourceUnavailable"
  | "permissionDenied"
  | "fileNotFound"
  | "fileChanged"
  | "unsupportedMedia"
  | "corruptMedia"
  | "ffprobeUnavailable"
  | "ffprobeFailed"
  | "ffmpegUnavailable"
  | "ffmpegFailed"
  | "mediaProcessTimeout"
  | "mediaProcessCancelled"
  | "databaseUnavailable"
  | "databaseBusy"
  | "databaseMigrationFailed"
  | "databaseIntegrityFailed"
  | "diskFull"
  | "destinationCollision"
  | "exportCancelled"
  | "exportFailed"
  | "thumbnailGenerationFailed"
  | "cacheOperationFailed"
  | "windowsIntegrationFailed"
  | "internal";

export interface AppErrorPayload {
  code: AppErrorCode;
  message: string;
  retryable: boolean;
}

export interface AppSettings {
  sourceFolders: string[];
  workingFolder?: string | null;
  clipFolder?: string | null;
  storageLimitBytes?: number | null;
  cleanupThresholdPercent: number;
  theme?: "system" | "dark" | "light";
  automaticallyCheckUpdates?: boolean;
  automaticallyDownloadUpdates?: boolean;
  includePrereleaseUpdates?: boolean;
  preferredRecordingApp?: string | null;
  automaticallyArchiveRecordings?: boolean;
  automaticallyArchiveClips?: boolean;
  keepLocalClips?: boolean;
  pauseUploadsWhileRecording?: boolean;
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
  version: string;
  settings: AppSettings;
  ffmpeg: ToolHealth;
  stats: StorageStats;
  activeMediaWork?: boolean;
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
  gameId?: string | null;
  gameName?: string | null;
  keepLocal?: boolean;
  availability?: "localOnly" | "queued" | "preparing" | "uploading" | "waitingForQuota" | "processingRemote" | "archivedVerified" | "remoteOnly" | "uploadFailed" | "remoteMissing" | "multipleRemoteCopies";
  remoteCopyCount?: number;
  primaryRemoteUrl?: string | null;
}

export interface Collection {
  id: string;
  name: string;
  itemCount: number;
  storageDefault: "keepLocal" | "archive" | null;
}

export interface Game {
  id: string;
  name: string;
  color: string;
  mediaCount: number;
  aliases: string[];
}

export interface GameAssignment {
  mediaId: string;
  gameId: string;
  gameName: string;
  color: string;
}

export interface ArchiveStatus {
  mediaId: string;
  state: NonNullable<MediaItem["availability"]>;
  keepLocal: boolean;
  remoteCopyCount: number;
  primaryRemoteUrl?: string | null;
  progressPercent: number;
  error?: string | null;
}
export interface ArchiveAccount { provider: string; accountId: string; displayName: string; connectedAt: number }

export interface UploadJob {
  id: string;
  mediaId: string;
  filename: string;
  state: string;
  priority: number;
  bytesUploaded: number;
  totalBytes: number;
  attempts: number;
  error?: string | null;
  updatedAt: number;
}

export interface MediaEvent { event: string; detail?: string | null; createdAt: number }
export interface RecorderApp { id: string; name: string; executablePath: string }
export interface RepairSummary { missingLocalFiles: number; preservedRemoteOnlyItems: number; thumbnailsQueued: number; staleJobsRecovered: number }

export interface LibraryDataExportSummary {
  items: number;
  collections: number;
  bytes: number;
}

export interface LibraryDataImportSummary {
  matchedItems: number;
  restoredItems: number;
  unmatchedItems: number;
  updatedItems: number;
  tagConflicts: number;
  noteConflicts: number;
  collectionsCreated: number;
  membershipsAdded: number;
}

export interface CreateClipRequest {
  sourceId: string;
  sessionId?: string | null;
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

export interface AudioPreviewResult {
  path: string;
  cacheHit: boolean;
  method: "cache" | "copy" | "transcode";
  elapsedMs: number;
  bytes: number;
}
