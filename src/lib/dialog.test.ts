import { open, save } from "@tauri-apps/plugin-dialog";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { pickLibraryDataExport, pickLibraryDataImport } from "./dialog";

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
  save: vi.fn()
}));

describe("portable library data dialogs", () => {
  beforeEach(() => vi.clearAllMocks());

  it("limits imports to a single JSON file", async () => {
    vi.mocked(open).mockResolvedValue("C:\\backup\\library.json");
    await expect(pickLibraryDataImport()).resolves.toBe("C:\\backup\\library.json");
    expect(open).toHaveBeenCalledWith(expect.objectContaining({
      directory: false,
      multiple: false,
      filters: [{ name: "Reelspace library data", extensions: ["json"] }]
    }));
  });

  it("provides a dated JSON export filename and handles cancellation", async () => {
    vi.mocked(save).mockResolvedValue(null);
    await expect(pickLibraryDataExport()).resolves.toBeNull();
    expect(save).toHaveBeenCalledWith(expect.objectContaining({
      defaultPath: expect.stringMatching(/^reelspace-library-data-\d{4}-\d{2}-\d{2}\.json$/),
      filters: [{ name: "Reelspace library data", extensions: ["json"] }]
    }));
  });
});
