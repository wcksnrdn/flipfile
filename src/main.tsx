import React, { Suspense, lazy } from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
// Poppins, bundled locally (Latin subset only) so the app looks the same offline
import "@fontsource/poppins/latin-400.css";
import "@fontsource/poppins/latin-500.css";
import "@fontsource/poppins/latin-600.css";

// The summary panel is a second window running this same page (src-tauri/src/summary/window.rs);
// its code only loads there
const SummaryPanel = lazy(() => import("./SummaryPanel"));
const isSummaryPanel = getCurrentWindow().label === "summary";
// The notification bubble likewise (src-tauri/src/toast.rs)
const ToastBubble = lazy(() => import("./ToastBubble"));
const isToast = getCurrentWindow().label === "toast";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {isSummaryPanel ? (
      <Suspense fallback={null}>
        <SummaryPanel />
      </Suspense>
    ) : isToast ? (
      <Suspense fallback={null}>
        <ToastBubble />
      </Suspense>
    ) : (
      <App />
    )}
  </React.StrictMode>,
);
