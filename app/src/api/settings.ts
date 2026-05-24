import { api } from "./client";
import type { ServerSettings } from "./types";

export function getSettings() {
  return api<ServerSettings>("/app/settings");
}

export function updateSettings(patch: Partial<Pick<ServerSettings, "custom_keywords" | "benchmark_companies">>) {
  return api<ServerSettings>("/app/settings", {
    method: "PUT",
    body: JSON.stringify(patch),
  });
}
