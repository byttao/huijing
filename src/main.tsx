import { StrictMode, useCallback, useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./index.css";
import App from "./App";
import { THEMES, ThemeProvider, pushLog, type Accent, type LogLevel, type ThemeMode } from "./ui-kit";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { api } from "./lib/api";
import type { SettingsView } from "./lib/types";

// Rust 端队列日志：在 React 渲染前就开始监听，写进「出图」页的日志通道。
// 页面还没挂载时（例如启动时恢复队列）写入的日志先缓冲，GeneratePage 挂载后补上。
if (isTauri()) {
  listen<{ level: LogLevel; message: string }>("queue-log", ({ payload }) => pushLog("generate", payload.message, payload.level));
}

const MODES: ThemeMode[] = ["light", "dark", "system"];

/**
 * 读设置并把主题交给 ThemeProvider 受控：设置读到前传 undefined（用 localStorage 里的），
 * 读到后以 settings.json 为准；任何切换（侧栏菜单、设置页）经回调写回设置文件，
 * Windows 便携版拷到别的机器后主题也跟着走。
 */
function Root() {
  const [view, setView] = useState<SettingsView | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  useEffect(() => {
    api.getSettings().then(setView).catch((err) => setLoadError(String(err)));
  }, []);

  const viewRef = useRef(view);
  viewRef.current = view;
  const saveTheme = useCallback((patch: { themeAccent?: string; themeMode?: string }) => {
    const current = viewRef.current;
    if (!current) return;
    const next = { ...current, settings: { ...current.settings, ...patch } };
    setView(next); // 先切过去，再写文件
    api.saveSettings(next.settings).then(setView).catch(() => {});
  }, []);

  const s = view?.settings;
  const accent = s?.themeAccent && Object.hasOwn(THEMES, s.themeAccent) ? (s.themeAccent as Accent) : undefined;
  const mode = s && MODES.includes(s.themeMode as ThemeMode) ? (s.themeMode as ThemeMode) : undefined;

  return (
    <ThemeProvider
      accent={accent}
      mode={mode}
      onAccentChange={(a) => saveTheme({ themeAccent: a })}
      onModeChange={(m) => saveTheme({ themeMode: m })}
    >
      <ErrorBoundary>
        <App view={view} setView={setView} loadError={loadError} />
      </ErrorBoundary>
    </ThemeProvider>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Root />
  </StrictMode>
);
