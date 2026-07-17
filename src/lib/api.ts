import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import type {
  AppSettings,
  AppSnapshot,
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
  listMedia: (kind?: MediaKind) => invoke<MediaItem[]>("list_media", { kind }),
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
