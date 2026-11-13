import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { PictureOutlined, SettingOutlined, ThunderboltOutlined } from "@ant-design/icons";
import { App as AntApp, Badge } from "antd";
import { AppShell, THEMES, ThemeMenu, ThemeToggle, cn, useTheme, type Accent, type ThemeMode } from "./ui-kit";
import { BrandMark } from "./components/BrandMark";
import { GeneratePage } from "./pages/GeneratePage";
import { GalleryPage } from "./pages/GalleryPage";
import { SettingsPage } from "./pages/SettingsPage";
import { api, errorText } from "./lib/api";
import { isActive } from "./lib/options";
import type { EditRequest, SettingsView, Task } from "./lib/types";
import { useTasks } from "./lib/useTasks";

type PageKey = "generate" | "gallery" | "settings";

export default function App() {
  const [page, setPage] = useState<PageKey>("generate");
  const { tasks, loaded: tasksLoaded } = useTasks();
  const [view, setView] = useState<SettingsView | null>(null);
  const [editRequest, setEditRequest] = useState<EditRequest | null>(null);
  // 变量名用 toast：App.useApp() 的 message，与静态 message 区分（也让 AI_GUIDE 自检的字面搜索通过）
  const { message: toast } = AntApp.useApp();

  useEffect(() => {
    api
      .getSettings()
      .then(setView)
      .catch((err) => toast.error(`读取设置失败：${errorText(err)}`));
  }, [toast]);

  useThemeSync(view, setView);

  const startEdit = useCallback(
    (task: Task) => {
      if (!task.files.length) {
        toast.warning("这条任务还没有本地图片，无法继续修改");
        return;
      }
      setEditRequest({ task, nonce: Date.now() });
      setPage("generate");
    },
    [toast]
  );

  const activeCount = tasks.filter((t) => isActive(t.status)).length;
  const galleryCount = tasks.filter((t) => t.status === "succeeded").length;

  const nav = useMemo(
    () => [
      {
        key: "generate" as const,
        label: "出图",
        icon: <ThunderboltOutlined />,
        section: "创作",
        // 侧栏可能就是主色（如森翠），徽标用侧栏反色块，与品牌标记一致
        badge: activeCount ? <Badge count={activeCount} size="small" classNames={{ indicator: "bg-sidebar-active-text text-sidebar shadow-none" }} /> : undefined,
      },
      {
        key: "gallery" as const,
        label: "图库",
        icon: <PictureOutlined />,
        badge: galleryCount ? <span className="text-sm text-sidebar-muted tabular-nums">{galleryCount}</span> : undefined,
      },
      { key: "settings" as const, label: "设置", icon: <SettingOutlined />, section: "应用" },
    ],
    [activeCount, galleryCount]
  );

  return (
    <AppShell
      brand={{ mark: <BrandMark />, name: "绘境", subtitle: "AI 出图工作台" }}
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
        <GeneratePage
          active={page === "generate"}
          tasks={tasks}
          tasksLoaded={tasksLoaded}
          settings={view}
          editRequest={editRequest}
          onEdit={startEdit}
          onGoSettings={() => setPage("settings")}
        />
      </div>
      <div className={cn("h-full", page !== "gallery" && "hidden")}>
        <GalleryPage tasks={tasks} onEdit={startEdit} />
      </div>
      <div className={cn("h-full", page !== "settings" && "hidden")}>
        <SettingsPage view={view} onSaved={setView} />
      </div>
    </AppShell>
  );
}

/**
 * 主题写进设置文件：ui-kit 的 ThemeProvider 把主题存在 localStorage，
 * Windows 便携版换机器后 WebView 数据不跟着走，所以再在 settings.json 里存一份。
 * 启动时设置里有值就以设置为准；之后任何切换（侧栏菜单、设置页）都写回设置。
 */
function useThemeSync(view: SettingsView | null, setView: (v: SettingsView) => void) {
  const { accent, setAccent, mode, setMode } = useTheme();
  const synced = useRef(false);
  const viewRef = useRef(view);
  viewRef.current = view;

  // 启动：设置里有主题就套用；还没有（首次运行）就把当前主题写进设置
  useEffect(() => {
    if (!view || synced.current) return;
    synced.current = true;
    const { themeAccent, themeMode } = view.settings;
    const validAccent = !!themeAccent && Object.hasOwn(THEMES, themeAccent);
    const validMode = themeMode === "light" || themeMode === "dark" || themeMode === "system";
    if (validAccent && themeAccent !== accent) setAccent(themeAccent as Accent);
    if (validMode && themeMode !== mode) setMode(themeMode as ThemeMode);
    if (!validAccent || !validMode) {
      api
        .saveSettings({ ...view.settings, themeAccent: validAccent ? themeAccent : accent, themeMode: validMode ? themeMode : mode })
        .then(setView)
        .catch(() => {});
    }
  }, [view, accent, mode, setAccent, setMode, setView]);

  // 之后的切换（侧栏菜单、设置页）写回设置。依赖里没有 view，所以设置加载本身不会触发这里
  useEffect(() => {
    const current = viewRef.current;
    if (!synced.current || !current) return;
    const { themeAccent, themeMode } = current.settings;
    if (themeAccent === accent && themeMode === mode) return;
    api
      .saveSettings({ ...current.settings, themeAccent: accent, themeMode: mode })
      .then(setView)
      .catch(() => {});
  }, [accent, mode, setView]);
}
