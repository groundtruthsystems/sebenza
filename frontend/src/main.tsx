import "./app.css";
import { createRoot } from "react-dom/client";
import App from "./App";
import EmptyProjects from "./lib/EmptyProjects";
import RegistryView from "./lib/RegistryView";
import { ensureProjectPrefix } from "./lib/api";
import { applyTheme } from "./lib/utils";
import { loadSavedTheme } from "./lib/utils";

async function start(): Promise<void> {
  const target = document.getElementById("app")!;
  applyTheme(loadSavedTheme());
  // Pick a project (or redirect to one) before mounting, so the per-project API
  // client has a valid `/<prefix>` base. With no projects, mount a guided empty
  // state instead of a dashboard whose every /api call would 404.
  const status = await ensureProjectPrefix();
  if (status === "redirecting") return;
  const root = createRoot(target);
  if (status === "registry") root.render(<RegistryView />);
  else root.render(status === "no-projects" ? <EmptyProjects /> : <App />);
}

void start();
