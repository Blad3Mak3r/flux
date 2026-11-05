import React, { Suspense, lazy } from "react";
import ReactDOM from "react-dom/client";
import "./styles.css";

// The launcher and the welcome window are the same bundle as the editor, picked by `?view=`.
// Each is loaded on its own, so the launcher does not wait for the editor's code.
const Launcher = lazy(() => import("./launcher/Launcher"));
const Welcome = lazy(() => import("./welcome/Welcome"));
const App = lazy(() => import("./App"));

// The webview's own menu (Back, Reload, Inspect…) is not for the user; the app opens its own.
window.addEventListener("contextmenu", (event) => event.preventDefault());

const view = new URLSearchParams(window.location.search).get("view");
const Screen = view === "launcher" ? Launcher : view === "welcome" ? Welcome : App;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Suspense fallback={null}>
      <Screen />
    </Suspense>
  </React.StrictMode>,
);
