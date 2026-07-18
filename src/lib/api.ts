import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import type {
  AppSettings,
  AppErrorCode,
  AppErrorPayload,
  AppSnapshot,
  ArchiveStatus,
  ArchiveAccount,
  AudioPreviewResult,
  AudioTrack,
  Collection,
  CreateClipRequest,
  LibraryDataExportSummary,
  LibraryDataImportSummary,
  MediaItem,
  MediaKind,
  Game,
  GameAssignment,
  MediaEvent,
  RecorderApp,
  RepairSummary,
  UploadJob,
  StorageStats
} from "./types";

const ERROR_CODES = new Set<AppErrorCode>([
  "invalidRequest", "invalidPath", "pathOutsideManagedRoots", "sourceUnavailable",
  "permissionDenied", "fileNotFound", "fileChanged", "unsupportedMedia", "corruptMedia",
  "ffprobeUnavailable", "ffprobeFailed", "ffmpegUnavailable", "ffmpegFailed",
  "mediaProcessTimeout", "mediaProcessCancelled", "databaseUnavailable", "databaseBusy",
  "databaseMigrationFailed", "databaseIntegrityFailed", "diskFull", "destinationCollision",
  "exportCancelled", "exportFailed", "thumbnailGenerationFailed", "cacheOperationFailed",
  "windowsIntegrationFailed", "internal"
]);

export class ReelspaceError extends Error implements AppErrorPayload {
  readonly code: AppErrorCode;
  readonly retryable: boolean;

  constructor(payload: AppErrorPayload) {
    super(payload.message);
    this.name = "ReelspaceError";
    this.code = payload.code;
    this.retryable = payload.retryable;
  }
}

export function normalizeAppError(value: unknown): ReelspaceError {
  if (value instanceof ReelspaceError) return value;
  if (value && typeof value === "object") {
    const candidate = value as Partial<AppErrorPayload>;
    if (
      typeof candidate.code === "string" && ERROR_CODES.has(candidate.code as AppErrorCode) &&
      typeof candidate.message === "string" && typeof candidate.retryable === "boolean"
    ) {
      return new ReelspaceError(candidate as AppErrorPayload);
    }
  }
  return new ReelspaceError({
    code: "internal",
    message: "An unexpected Reelspace error occurred",
    retryable: false
  });
}

async function invokeCommand<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw normalizeAppError(error);
  }
}

export function mediaSrc(path?: string | null): string | undefined {
  return path ? convertFileSrc(path) : undefined;
}

export const api = {
  getAppState: () => invokeCommand<AppSnapshot>("get_app_state"),
  saveSettings: (settings: AppSettings) => invokeCommand<AppSnapshot>("save_settings", { settings }),
  scanSources: () => invokeCommand<MediaItem[]>("scan_sources"),
  repairThumbnails: () => invokeCommand<MediaItem[]>("repair_thumbnails"),
  listMedia: (kind?: MediaKind) => invokeCommand<MediaItem[]>("list_media", { kind }),
  getMediaAudioTracks: (id: string, sessionId: string) =>
    invokeCommand<AudioTrack[]>("get_media_audio_tracks", { id, sessionId }),
  getAudioTrackPreview: (id: string, streamIndex: number, sessionId: string) =>
    invokeCommand<AudioPreviewResult>("get_audio_track_preview", { id, streamIndex, sessionId }),
  cancelAudioPreviewSession: (sessionId: string) =>
    invokeCommand<void>("cancel_audio_preview_session", { sessionId }),
  logClientEvent: (event: string, fields: Record<string, unknown>) =>
    invokeCommand<void>("log_client_event", { event, fields }),
  updateMediaTags: (id: string, tags: string[]) =>
    invokeCommand<MediaItem>("update_media_tags", { id, tags }),
  updateMediaNotes: (id: string, notes?: string | null) =>
    invokeCommand<MediaItem>("update_media_notes", { id, notes }),
  listCollections: () => invokeCommand<Collection[]>("list_collections"),
  listCollectionMedia: (collectionId: string) =>
    invokeCommand<MediaItem[]>("list_collection_media", { collectionId }),
  createCollection: (name: string) => invokeCommand<Collection[]>("create_collection", { name }),
  addToCollection: (collectionId: string, mediaId: string) =>
    invokeCommand<Collection[]>("add_to_collection", { collectionId, mediaId }),
  exportLibraryData: (path: string) =>
    invokeCommand<LibraryDataExportSummary>("export_library_data", { path }),
  importLibraryData: (path: string) =>
    invokeCommand<LibraryDataImportSummary>("import_library_data", { path }),
  createClip: (request: CreateClipRequest) => invokeCommand<MediaItem>("create_clip", { request }),
  revealInExplorer: (id: string) => invokeCommand<void>("reveal_in_explorer", { id }),
  getStorageStats: () => invokeCommand<StorageStats>("get_storage_stats"),
  getCleanupCandidates: () => invokeCommand<MediaItem[]>("get_cleanup_candidates"),
  deleteRawFile: (id: string) => invokeCommand<MediaItem[]>("delete_raw_file", { id }),
  listGames: () => invokeCommand<Game[]>("list_games"),
  createGame: (name: string) => invokeCommand<Game[]>("create_game", { name }),
  listGameAssignments: () => invokeCommand<GameAssignment[]>("list_game_assignments"),
  renameGame: (id: string, name: string, preserveAlias = true) =>
    invokeCommand<Game[]>("rename_game", { id, name, preserveAlias }),
  mergeGames: (sourceId: string, targetId: string) =>
    invokeCommand<Game[]>("merge_games", { sourceId, targetId }),
  assignGame: (mediaId: string, gameId: string) =>
    invokeCommand<GameAssignment[]>("assign_game", { mediaId, gameId }),
  listArchiveStatuses: () => invokeCommand<ArchiveStatus[]>("list_archive_statuses"),
  setKeepLocal: (mediaId: string, keepLocal: boolean) =>
    invokeCommand<ArchiveStatus[]>("set_keep_local", { mediaId, keepLocal }),
  queueArchive: (mediaIds: string[]) => invokeCommand<number>("queue_archive", { mediaIds }),
  listUploadJobs: () => invokeCommand<UploadJob[]>("list_upload_jobs"),
  controlUploadQueue: (action: string, jobId?: string | null) =>
    invokeCommand<UploadJob[]>("control_upload_queue", { action, jobId }),
  listMediaEvents: (mediaId: string) =>
    invokeCommand<MediaEvent[]>("list_media_events", { mediaId }),
  removeFromCollection: (collectionId: string, mediaId: string) =>
    invokeCommand<void>("remove_from_collection", { collectionId, mediaId }),
  setCollectionStorageDefault: (
    collectionId: string,
    policy: "keepLocal" | "archive" | null,
  ) => invokeCommand<Collection[]>("set_collection_storage_default", { collectionId, policy }),
  detectRecordingApps: () => invokeCommand<RecorderApp[]>("detect_recording_apps"),
  launchRecordingApp: (id: string) => invokeCommand<void>("launch_recording_app", { id }),
  removeLocalCopy: (mediaId: string) =>
    invokeCommand<ArchiveStatus[]>("remove_local_copy", { mediaId }),
  openRemoteCopy: (mediaId: string) => invokeCommand<void>("open_remote_copy", { mediaId }),
  repairLibrary: () => invokeCommand<RepairSummary>("repair_library"),
  getArchiveAccount: () => invokeCommand<ArchiveAccount | null>("get_archive_account"),
  connectYouTube: () => invokeCommand<ArchiveAccount>("connect_youtube"),
  disconnectYouTube: () => invokeCommand<void>("disconnect_youtube"),
  deleteYouTubeRemote: (mediaId: string) =>
    invokeCommand<void>("delete_youtube_remote", { mediaId }),
  openDiagnosticsFolder: () => invokeCommand<void>("open_diagnostics_folder"),
  exportDiagnostics: (path: string) => invokeCommand<void>("export_diagnostics", { path }),
};
