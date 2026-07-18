import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { MediaItem } from "../lib/types";
import { MEDIA_RENDER_PAGE_SIZE, MediaLibrary } from "./MediaLibrary";

const actions = {
  view: () => undefined,
  trim: () => undefined,
  reveal: () => undefined,
  notes: () => undefined,
  tags: () => undefined,
  addToCollection: () => undefined
};

function item(index: number): MediaItem {
  return {
    id: index.toString(16).padStart(64, "0"),
    kind: "raw",
    path: `C:\\captures\\video-${index}.mp4`,
    filename: `video-${index}.mp4`,
    fileSize: index + 1,
    durationSeconds: 10,
    tags: [],
    createdAt: index,
    modifiedAt: index
  };
}

describe("MediaLibrary high-item-count rendering", () => {
  it("bounds the initial DOM while preserving the full result count", () => {
    const total = MEDIA_RENDER_PAGE_SIZE + 50;
    const markup = renderToStaticMarkup(
      <MediaLibrary
        items={Array.from({ length: total }, (_, index) => item(index))}
        actions={actions}
        empty="No media"
      />
    );
    expect(markup.match(/class="media-card /g)).toHaveLength(MEDIA_RENDER_PAGE_SIZE);
    expect(markup).toContain(`<strong>${total}</strong>`);
    expect(markup).toContain("Show 50 more");
  });
});
