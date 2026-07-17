import { open } from "@tauri-apps/plugin-dialog";

export async function pickFolder(): Promise<string | null> {
  const selected = await open({
    directory: true,
    multiple: false,
    title: "Choose capture folder"
  });
  return typeof selected === "string" ? selected : null;
}
