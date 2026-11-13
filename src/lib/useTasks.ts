// 任务列表：首次拉取 + 订阅 Rust 端推送的 task-updated / task-removed
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import type { Task } from "./types";

export function useTasks() {
  const [tasks, setTasks] = useState<Task[]>([]);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    let disposed = false;
    const unlisteners: Array<() => void> = [];
    const keep = (fn: () => void) => (disposed ? fn() : unlisteners.push(fn));

    listen<Task>("task-updated", ({ payload }) => {
      setTasks((prev) => {
        const index = prev.findIndex((t) => t.id === payload.id);
        if (index < 0) return [...prev, payload];
        const next = prev.slice();
        next[index] = payload;
        return next;
      });
    }).then(keep);
    listen<string[]>("task-removed", ({ payload }) => {
      const removed = new Set(payload);
      setTasks((prev) => prev.filter((t) => !removed.has(t.id)));
    }).then(keep);

    api
      .listTasks()
      .then((list) => !disposed && setTasks(list))
      .finally(() => !disposed && setLoaded(true));

    return () => {
      disposed = true;
      unlisteners.forEach((fn) => fn());
    };
  }, []);

  return { tasks, loaded };
}
