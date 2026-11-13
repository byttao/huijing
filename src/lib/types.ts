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
  /** Nano 多合一时用高一档分辨率请求 */
  higherRes: boolean;
  /** 多合一 2 / 3 张的排法："row" 横排、"column" 竖排，空为默认 */
  layout: string;
}

/** 宫格方案（与 Rust 端 grid::GridPlan 一致） */
export interface GridPlan {
  count: number;
  cols: number;
  rows: number;
  /** 2 / 3 张时 "row" 横排、"column" 竖排；单张与 4 张为空 */
  orientation: string;
  /** 整张图（实际请求）的比例，就是所选比例 */
  requestRatio: string;
  /** 拆切后每张的比例（约分）；比例为自动时为空 */
  cellRatio: string;
  requestTier: string;
  tier: string;
  requestSize: [number, number];
  cellSize: [number, number];
  outputSize: [number, number];
  upscale: boolean;
  estimated: boolean;
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
  /** 多合一张数，1 = 普通单张 */
  count: number;
  grid: GridPlan | null;
  /** 实际发送的提示词（多合一时外包宫格指令）；界面显示 prompt 原文 */
  sentPrompt: string | null;
  /** 多合一的合成原图；files 是拆切后的各张 */
  compositeFile: string | null;
  notice: string | null;
  /** 持久自增序号 #12 */
  number: number;
  /** 改自的原记录序号与切片号（#7-2） */
  parentNumber: number | null;
  parentPart: number | null;
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
  /** 多合一拆切后放大到单张原尺寸 */
  upscaleParts: boolean;
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
  /** 作为参考图的那一张（多合一时是拆切后的某一张） */
  file: string;
  nonce: number;
}
