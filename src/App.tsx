import { useCallback, useMemo, useState } from "react";
import { PictureOutlined, SettingOutlined, ThunderboltOutlined } from "@ant-design/icons";
import { App as AntApp } from "antd";
import { AppShell, ErrorBoundary, NavBadge, ThemeMenu, ThemeToggle, cn, useTaskEvents } from "./ui-kit";
import brandMark from "./assets/brand-mark.png";
import { GeneratePage } from "./pages/GeneratePage";
import { GalleryPage } from "./pages/GalleryPage";
import { SettingsPage } from "./pages/SettingsPage";
import { taskSource } from "./lib/taskSource";
import { isActive } from "./lib/options";
import type { EditRequest, SettingsView, Task } from "./lib/types";

type PageKey = "generate" | "gallery" | "settings";

interface AppProps {
  view: SettingsView | null;
  setView: (view: SettingsView) => void;
}

export default function App({ view, setView }: AppProps) {
  const [page, setPage] = useState<PageKey>("generate");
  // 先订阅 task-updated / task-removed 再加载列表，加载期间的更新不会丢
  const { tasks, loaded: tasksLoaded } = useTaskEvents<Task>(taskSource);
  const [editRequest, setEditRequest] = useState<EditRequest | null>(null);
  const [galleryFocus, setGalleryFocus] = useState<{ taskId: string; file?: string; nonce: number } | null>(null);
  const { message } = AntApp.useApp();


  const startEdit = useCallback(
    (task: Task, file?: string) => {
      if (!task.files.length) {
        message.warning("这条任务还没有本地图片，无法继续修改");
        return;
      }
      setEditRequest({ task, file: file ?? task.files[0], nonce: Date.now() });
      setPage("generate");
    },
    [message]
  );

  const activeCount = tasks.filter((t) => isActive(t.status)).length;
  const galleryCount = tasks.reduce((n, t) => n + (t.status === "succeeded" ? t.files.length : 0), 0);

  const nav = useMemo(
    () => [
      { key: "generate" as const, label: "出图", icon: <ThunderboltOutlined />, section: "创作", badge: <NavBadge count={activeCount} /> },
      { key: "gallery" as const, label: "图库", icon: <PictureOutlined />, badge: <NavBadge count={galleryCount} tone="muted" /> },
      { key: "settings" as const, label: "设置", icon: <SettingOutlined />, section: "应用" },
    ],
    [activeCount, galleryCount]
  );

  return (
    <AppShell
      brand={{ mark: { src: brandMark, alt: "绘境" }, name: "绘境", subtitle: "AI 出图工作台" }}
      nav={nav}
      active={page}
      onNavigate={setPage}
      sidebarFooter={
        <>
          <span>版本 V{view?.version ?? "0.1.0"}</span>
          <span className="flex items-center gap-1">
            <ThemeMenu />
            <ThemeToggle />
          </span>
        </>
      }
    >
      {/* 三个页面常驻，切换时只隐藏：各页的日志、草稿与事件订阅不丢 */}
      <div className={cn("h-full", page !== "generate" && "hidden")}>
        <ErrorBoundary size="page">
          <GeneratePage
            active={page === "generate"}
            tasks={tasks}
            tasksLoaded={tasksLoaded}
            settings={view}
            editRequest={editRequest}
            onEdit={startEdit}
            onGoSettings={() => setPage("settings")}
            onShowInGallery={(task, file) => {
              setGalleryFocus({ taskId: task.id, file, nonce: Date.now() });
              setPage("gallery");
              message.info(`#${task.number} 已从队列清除，在图库中定位`);
            }}
          />
        </ErrorBoundary>
      </div>
      <div className={cn("h-full", page !== "gallery" && "hidden")}>
        <ErrorBoundary size="page">
            <GalleryPage tasks={tasks} onEdit={startEdit} focus={galleryFocus} />
        </ErrorBoundary>
      </div>
      <div className={cn("h-full", page !== "settings" && "hidden")}>
        <ErrorBoundary size="page">
            <SettingsPage view={view} onSaved={setView} />
        </ErrorBoundary>
      </div>
    </AppShell>
  );
}
