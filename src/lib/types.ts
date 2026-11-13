// 与 Rust 端（src-tauri/src）序列化结构一一对应

export type ModelId = "gpt-image-2.6" | "nano-banana-2.1";
export type Mode = "text" | "image" | "edit";
export type TaskStatus =
  | "queued"
  | "generating"
  | "downloading"
  | "succeeded"
  | "failed"
  | "download_failed"
  | "cancelled";

export interface GenParams {
  /** 比例：auto、1:1、16:9 … */
  aspectRatio: string;
  /** Nano 分辨率：1K / 2K / 4K */
  imageSize: string;
  /** GPT 背景："" 默认 / "transparent" 透明 */
  background: string;
  /** GPT 输出格式：png / gif / svg */
  outputFormat: string;
}

export interface Task {
  id: string;
  batchId: string;
  seq: number;
  parentId: string | null;
  mode: Mode;
  model: ModelId;
  prompt: string;
  params: GenParams;
  refImages: string[];
  status: TaskStatus;
  progress: number;
  remoteId: string | null;
  resultUrls: string[];
  files: string[];
  error: string | null;
  createdAt: number;
  startedAt: number | null;
  finishedAt: number | null;
  attempts: number;
  inQueue: boolean;
}

export type ProxyMode = "none" | "system" | "custom";

export interface Settings {
  apiKey: string;
  node: "cn" | "global";
  proxy: { mode: ProxyMode; url: string; forApi: boolean; forDownload: boolean };
  outputDir: string;
  concurrency: number;
  defaultModel: ModelId;
  gptParams: GenParams;
  nanoParams: GenParams;
  defaultCount: number;
  themeAccent: string;
  themeMode: string;
}

export interface SettingsView {
  settings: Settings;
  outputDir: string;
  defaultOutputDir: string;
  dataDir: string;
  version: string;
  portable: boolean;
}

export interface EnqueueRequest {
  mode: Mode;
  model: ModelId;
  prompt: string;
  params: GenParams;
  refImages: string[];
  count: number;
  parentId: string | null;
}

/** 「继续修改」：从队列或图库带到出图页的来源任务 */
export interface EditRequest {
  task: Task;
  nonce: number;
}
