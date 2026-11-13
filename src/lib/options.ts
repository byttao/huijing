// 模型、比例、状态等展示用的常量
import type { GenParams, Mode, ModelId, TaskStatus } from "./types";

export const MODELS: { value: ModelId; label: string; hint: string }[] = [
  { value: "nano-banana-2.1", label: "Nano Banana 2.1", hint: "异步出图，带进度，支持 1K / 2K / 4K" },
  { value: "gpt-image-2.6", label: "GPT Image 2.6", hint: "同步出图，支持透明背景与 GIF / SVG" },
];

export const modelLabel = (id: ModelId) => MODELS.find((m) => m.value === id)?.label ?? id;

export const MODES: { value: Mode; label: string }[] = [
  { value: "text", label: "文生图" },
  { value: "image", label: "图生图" },
  { value: "edit", label: "继续修改" },
];

export const modeLabel = (mode: Mode) => MODES.find((m) => m.value === mode)?.label ?? mode;

/** GPT：比例 → 1K 像素尺寸（与 Rust 端 gpt_image::SIZES 一致） */
export const GPT_RATIOS: { value: string; size: string }[] = [
  { value: "auto", size: "自动" },
  { value: "1:1", size: "1024×1024" },
  { value: "16:9", size: "1672×941" },
  { value: "9:16", size: "941×1672" },
  { value: "4:3", size: "1443×1090" },
  { value: "3:4", size: "1090×1443" },
  { value: "3:2", size: "1536×1024" },
  { value: "2:3", size: "1024×1536" },
  { value: "5:4", size: "1408×1120" },
  { value: "4:5", size: "1120×1408" },
  { value: "21:9", size: "1920×832" },
  { value: "9:21", size: "832×1920" },
  { value: "2:1", size: "1792×896" },
  { value: "1:2", size: "896×1792" },
];

export const NANO_RATIOS = ["auto", "1:1", "16:9", "9:16", "4:3", "3:4", "3:2", "2:3", "5:4", "4:5", "21:9", "1:4", "4:1", "1:8", "8:1"];

/** "a:b" → [a, b]；auto 返回 null */
export function parseRatio(r: string): [number, number] | null {
  const m = /^(\d+):(\d+)$/.exec(r);
  return m ? [Number(m[1]), Number(m[2])] : null;
}

export const IMAGE_SIZES = ["1K", "2K", "4K"];
export const BACKGROUNDS = [
  { value: "", label: "默认" },
  { value: "transparent", label: "透明" },
];
export const OUTPUT_FORMATS = [
  { value: "png", label: "PNG" },
  { value: "gif", label: "GIF" },
  { value: "svg", label: "SVG" },
];

/** 参数摘要，用于任务行与图库 */
export const sizeText = ([w, h]: [number, number]) => (w && h ? `${w}×${h}` : "自动");

export function paramsSummary(model: ModelId, p: GenParams) {
  const ratio = p.aspectRatio === "auto" || !p.aspectRatio ? "自动比例" : p.aspectRatio;
  if (model === "nano-banana-2.1") return `${ratio} · ${p.imageSize || "1K"}`;
  const extra = [p.background === "transparent" ? "透明背景" : "", p.outputFormat && p.outputFormat !== "png" ? p.outputFormat.toUpperCase() : ""];
  return [ratio, ...extra].filter(Boolean).join(" · ");
}

export const ACTIVE: TaskStatus[] = ["queued", "generating", "downloading"];
export const isActive = (s: TaskStatus) => ACTIVE.includes(s);

export function formatTime(ms: number) {
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

export function shortPrompt(prompt: string, max = 18) {
  return prompt.length > max ? `${prompt.slice(0, max)}…` : prompt;
}
