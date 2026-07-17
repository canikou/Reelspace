import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import type {
  AppSettings,
  AppSnapshot,
  AudioPreviewResult,
  AudioTrack,
  Collection,
  CreateClipRequest,
  MediaItem,
  MediaKind,
  StorageStats
} from "./types";

export function mediaSrc(path?: string | null): string | undefined {
  return path ? convertFileSrc(path) : undefined;
}

export const api = {
  getAppState: () => invoke<AppSnapshot>("get_app_state"),
  saveSettings: (settings: AppSettings) => invoke<AppSnapshot>("save_settings", { settings }),
  scanSources: () => invoke<MediaItem[]>("scan_sources"),
  repairThumbnails: () => invoke<MediaItem[]>("repair_thumbnails"),
  listMedia: (kind?: MediaKind) => invoke<MediaItem[]>("list_media", { kind }),
  getMediaAudioTracks: (id: string, sessionId: string) =>
    invoke<AudioTrack[]>("get_media_audio_tracks", { id, sessionId }),
  getAudioTrackPreview: (id: string, streamIndex: number, sessionId: string) =>
    invoke<AudioPreviewResult>("get_audio_track_preview", { id, streamIndex, sessionId }),
  cancelAudioPreviewSession: (sessionId: string) =>
    invoke<void>("cancel_audio_preview_session", { sessionId }),
  logClientEvent: (event: string, fields: Record<string, unknown>) =>
    invoke<void>("log_client_event", { event, fields }),
  updateMediaTags: (id: string, tags: string[]) =>
    invoke<MediaItem>("update_media_tags", { id, tags }),
  updateMediaNotes: (id: string, notes?: string | null) =>
    invoke<MediaItem>("update_media_notes", { id, notes }),
  listCollections: () => invoke<Collection[]>("list_collections"),
  listCollectionMedia: (collectionId: string) =>
    invoke<MediaItem[]>("list_collection_media", { collectionId }),
  createCollection: (name: string) => invoke<Collection[]>("create_collection", { name }),
  addToCollection: (collectionId: string, mediaId: string) =>
    invoke<Collection[]>("add_to_collection", { collectionId, mediaId }),
  createClip: (request: CreateClipRequest) => invoke<MediaItem>("create_clip", { request }),
  revealInExplorer: (path: string) => invoke<void>("reveal_in_explorer", { path }),
  getStorageStats: () => invoke<StorageStats>("get_storage_stats"),
  getCleanupCandidates: () => invoke<MediaItem[]>("get_cleanup_candidates"),
  deleteRawFile: (id: string) => invoke<MediaItem[]>("delete_raw_file", { id })
};
