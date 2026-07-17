import { Component, type ErrorInfo, type ReactNode } from "react";
import { api } from "../lib/api";

interface Props {
  children: ReactNode;
}

interface State {
  failed: boolean;
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = { failed: false };

  static getDerivedStateFromError(): State {
    return { failed: true };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    void api.logClientEvent("ui.render.failed", {
      message: error.message.slice(0, 1000),
      componentStack: info.componentStack?.slice(0, 4000) ?? "unavailable"
    }).catch(() => undefined);
  }

  render() {
    if (!this.state.failed) return this.props.children;
    return (
      <main className="recovery-screen" role="alert">
        <div>
          <p className="eyebrow">Reelspace recovered a display failure</p>
          <h1>Your media files were not changed</h1>
          <p>The failing screen was isolated. Reload Reelspace to rebuild its in-memory view from the local library.</p>
          <button onClick={() => window.location.reload()}>Reload Reelspace</button>
        </div>
      </main>
    );
  }
}
