import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
// Poppins, bundled locally (Latin subset only) so the app looks the same offline
import "@fontsource/poppins/latin-400.css";
import "@fontsource/poppins/latin-500.css";
import "@fontsource/poppins/latin-600.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
