import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { AppSnapshot } from "../lib/types";
import { SettingsPage } from "./SettingsPage";

const snapshot: AppSnapshot = {
  version: "0.1.1-alpha.1",
  settings: {
    sourceFolders: ["C:\\captures"],
    workingFolder: null,
    clipFolder: null,
    storageLimitBytes: null,
    cleanupThresholdPercent: 90
  },
  ffmpeg: {
    ready: true,
    message: "Ready",
    ffmpegPath: "C:\\tools\\ffmpeg.exe",
    ffprobePath: "C:\\tools\\ffprobe.exe"
  },
  stats: {
    driveTotalBytes: 1_000,
    driveAvailableBytes: 500,
    rawBytes: 100,
    clipBytes: 20,
    managedBytes: 120,
    storageLimitBytes: null,
    isNearLimit: false
  }
};

describe("SettingsPage library data controls", () => {
  it("explains the metadata-only archive and exposes import and export", () => {
    const markup = renderToStaticMarkup(
      <SettingsPage
        snapshot={snapshot}
        onSave={async () => undefined}
        onExportData={async () => ({ items: 1, collections: 1, bytes: 100 })}
        onImportData={async () => ({
          matchedItems: 1,
          restoredItems: 0,
          unmatchedItems: 0,
          updatedItems: 1,
          tagConflicts: 0,
          noteConflicts: 0,
          collectionsCreated: 1,
          membershipsAdded: 1
        })}
      />
    );
    expect(markup).toContain("Library data");
    expect(markup).toContain("without copying video, thumbnails, or previews");
    expect(markup).toContain("Export Library Data");
    expect(markup).toContain("Import Library Data");
    expect(markup).toContain("Reelspace 0.1.1-alpha.1");
  });
});
