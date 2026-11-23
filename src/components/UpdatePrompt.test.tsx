import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { UpdatePrompt } from "./UpdatePrompt";

const noop = () => undefined;
const render = (state: Parameters<typeof UpdatePrompt>[0]["state"]) =>
  renderToStaticMarkup(<UpdatePrompt state={state} activeWork={false} onDownload={noop} onRetry={noop} onInstall={noop} onLater={noop} onSkip={noop} />);

describe("UpdatePrompt", () => {
  it("offers Download for an available update", () => {
    const markup = render({ phase: "available", version: "1.2.3" });
    expect(markup).toContain("Reelspace 1.2.3 is available");
    expect(markup).toContain("Download");
    expect(markup).not.toContain("Retry");
  });

  it("shows the error and Retry, not Download, after a failed check", () => {
    const markup = render({ phase: "error", version: "1.2.3", error: "The update channel could not be reached" });
    expect(markup).toContain("The update channel could not be reached");
    expect(markup).toContain("Retry");
    expect(markup).not.toContain("Download");
    expect(markup).not.toContain("is available");
  });
});
