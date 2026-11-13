// 后端命令封装：页面只调这里，不直接写命令名
import { invoke } from "@tauri-apps/api/core";
import type { EnqueueRequest, GenParams, GridPlan, ModelId, Settings, SettingsView, Task } from "./types";

export const api = {
  getSettings: () => invoke<SettingsView>("get_settings"),
  saveSettings: (settings: Settings) => invoke<SettingsView>("save_settings", { settings }),
  testConnection: (settings: Settings) => invoke<string>("test_connection", { settings }),
  planGrid: (model: ModelId, params: GenParams, count: number) => invoke<GridPlan>("plan_grid", { model, params, count }),
  listTasks: () => invoke<Task[]>("list_tasks"),
  enqueue: (request: EnqueueRequest) => invoke<Task[]>("enqueue", { request }),
  cancel: (id: string) => invoke<void>("cancel_task", { id }),
  retry: (id: string) => invoke<void>("retry_task", { id }),
  retryDownload: (id: string) => invoke<void>("retry_download", { id }),
  remove: (ids: string[]) => invoke<void>("delete_tasks", { ids }),
  clearFinished: () => invoke<number>("clear_finished"),
  openFolder: (path: string) => invoke<void>("open_folder", { path }),
  revealFile: (path: string) => invoke<void>("reveal_file", { path }),
};

/** invoke 抛出的错误是字符串或 Error，统一成文字 */
export function errorText(err: unknown) {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return String(err);
}
