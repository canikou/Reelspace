import { open, save } from "@tauri-apps/plugin-dialog";

export async function pickFolder(): Promise<string | null> {
  const selected = await open({
    directory: true,
    multiple: false,
    title: "Choose capture folder"
  });
  return typeof selected === "string" ? selected : null;
}

const libraryDataFilters = [{ name: "Reelspace library data", extensions: ["json"] }];

export async function pickLibraryDataImport(): Promise<string | null> {
  const selected = await open({
    directory: false,
    multiple: false,
    title: "Import Reelspace library data",
    filters: libraryDataFilters
  });
  return typeof selected === "string" ? selected : null;
}

export async function pickLibraryDataExport(): Promise<string | null> {
  const date = new Date().toISOString().slice(0, 10);
  const selected = await save({
    title: "Export Reelspace library data",
    defaultPath: `reelspace-library-data-${date}.json`,
    filters: libraryDataFilters
  });
  return typeof selected === "string" ? selected : null;
}
