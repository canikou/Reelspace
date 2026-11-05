import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { api, initMediaSrc } from "./lib/api";
import "./styles.css";

window.addEventListener("error", (event) => {
  void api.logClientEvent("ui.window.error", {
    message: event.message.slice(0, 1000),
    filename: event.filename ? "application-script" : "unavailable",
    line: event.lineno,
    column: event.colno
  }).catch(() => undefined);
});

window.addEventListener("unhandledrejection", (event) => {
  const message = event.reason instanceof Error ? event.reason.message : "Unhandled promise rejection";
  void api.logClientEvent("ui.promise.unhandled", { message: message.slice(0, 1000) })
    .catch(() => undefined);
});

void initMediaSrc().then(() => ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>
));
