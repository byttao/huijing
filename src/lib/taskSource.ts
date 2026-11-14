// 任务列表的数据来源：Tauri 里订阅 Rust 推送的 task-updated / task-removed 并 invoke list_tasks；
// 浏览器里（Vite 预览、页面测试）用 kit 的 createMockTaskBackend，数据取自 window.__HUIJING_MOCK_TASKS__，
// 不必伪造 window.isTauri。
import { isTauri } from "@tauri-apps/api/core";
import { createMockTaskBackend, type UseTaskEventsOptions } from "../ui-kit";
import { api } from "./api";
import type { Task } from "./types";

declare global {
  interface Window {
    __HUIJING_MOCK_TASKS__?: Task[];
  }
}

function browserSource(): UseTaskEventsOptions<Task> {
  const backend = createMockTaskBackend<Task>(window.__HUIJING_MOCK_TASKS__ ?? []);
  return { listen: backend.listen, load: backend.load };
}

export const taskSource: UseTaskEventsOptions<Task> = isTauri() ? { load: api.listTasks } : browserSource();
