import { useEffect, useMemo, useRef, useState } from "react";
import { ClearOutlined, PictureOutlined, SettingOutlined } from "@ant-design/icons";
import { Alert, App as AntApp, Button, Empty, Form, Input, Segmented, Select, Typography } from "antd";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  DropZone,
  IMAGE_EXTENSIONS,
  ListWell,
  LogViewer,
  Page,
  Panel,
  Workspace,
  fileName,
  useFileDrop,
  useLogger,
  type LogLevel,
} from "../ui-kit";
import { RefImages } from "../components/RefImages";
import { TaskRow } from "../components/TaskRow";
import { Thumb } from "../components/Thumb";
import { api, errorText } from "../lib/api";
import {
  BACKGROUNDS,
  IMAGE_SIZES,
  MODELS,
  MODES,
  OUTPUT_FORMATS,
  gptRatioOptions,
  isActive,
  nanoRatioOptions,
  shortPrompt,
} from "../lib/options";
import type { EditRequest, GenParams, Mode, ModelId, SettingsView, Task } from "../lib/types";

const IS_MAC = /Mac/i.test(navigator.userAgent);
const COUNTS = [1, 2, 3, 4].map((n) => ({ value: n, label: `${n} 张` }));
const MAX_REFS = 8;

interface GeneratePageProps {
  active: boolean;
  tasks: Task[];
  tasksLoaded: boolean;
  settings: SettingsView | null;
  editRequest: EditRequest | null;
  onEdit: (task: Task) => void;
  onGoSettings: () => void;
}

export function GeneratePage({ active, tasks, tasksLoaded, settings, editRequest, onEdit, onGoSettings }: GeneratePageProps) {
  // 变量名用 toast：App.useApp() 的 message，与静态 message 区分（也让 AI_GUIDE 自检的字面搜索通过）
  const { message: toast, modal } = AntApp.useApp();
  const { logs, log, clear } = useLogger();

  const [mode, setMode] = useState<Mode>("text");
  const [model, setModel] = useState<ModelId>("nano-banana-2.1");
  const [prompt, setPrompt] = useState("");
  const [gptParams, setGptParams] = useState<GenParams>({ aspectRatio: "1:1", imageSize: "1K", background: "", outputFormat: "png" });
  const [nanoParams, setNanoParams] = useState<GenParams>({ aspectRatio: "auto", imageSize: "1K", background: "", outputFormat: "png" });
  const [count, setCount] = useState(1);
  const [refs, setRefs] = useState<string[]>([]);
  const [editSource, setEditSource] = useState<Task | null>(null);
  const [submitting, setSubmitting] = useState(false);

  // 首次拿到设置时套用默认模型与参数
  const initialized = useRef(false);
  useEffect(() => {
    if (!settings || initialized.current) return;
    initialized.current = true;
    const s = settings.settings;
    setModel(s.defaultModel);
    setGptParams(s.gptParams);
    setNanoParams(s.nanoParams);
    setCount(s.defaultCount);
  }, [settings]);

  // 「继续修改」：沿用来源任务的模型与参数，结果图作为参考图
  useEffect(() => {
    if (!editRequest) return;
    const { task } = editRequest;
    setEditSource(task);
    setMode("edit");
    setModel(task.model);
    if (task.model === "gpt-image-2.6") setGptParams(task.params);
    else setNanoParams(task.params);
    setPrompt(task.prompt);
    log(`继续修改「${shortPrompt(task.prompt)}」：已带入结果图与原参数，改好提示词后加入队列。`);
  }, [editRequest, log]);

  // Rust 端队列日志
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    listen<{ level: LogLevel; message: string }>("queue-log", ({ payload }) => log(payload.message, payload.level)).then((fn) =>
      disposed ? fn() : (unlisten = fn)
    );
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [log]);

  // 首次载入：说明历史记录与从上次继续的任务（Rust 端启动日志发出时页面还没开始监听）
  const announced = useRef(false);
  useEffect(() => {
    if (!tasksLoaded || announced.current) return;
    announced.current = true;
    if (!tasks.length) return;
    const resumed = tasks.filter((t) => t.status === "generating" || t.status === "downloading").length;
    const queued = tasks.filter((t) => t.status === "queued").length;
    const parts = [`已载入 ${tasks.length} 条任务记录`];
    if (resumed) parts.push(`${resumed} 个从上次中断处继续`);
    if (queued) parts.push(`${queued} 个排队中`);
    log(`${parts.join("，")}。`);
  }, [tasksLoaded, tasks, log]);

  const addRefs = (paths: string[]) => {
    const fresh = paths.filter((p) => !refs.includes(p));
    if (!fresh.length) return;
    const room = MAX_REFS - refs.length;
    if (room <= 0) {
      toast.warning(`参考图最多 ${MAX_REFS} 张`);
      return;
    }
    const taken = fresh.slice(0, room);
    setRefs((prev) => [...prev, ...taken]);
    if (mode !== "image") setMode("image");
    log(`已添加 ${taken.length} 张参考图：${taken.map(fileName).join("、")}`);
    if (taken.length < fresh.length) toast.warning(`参考图最多 ${MAX_REFS} 张，多出的已忽略`);
  };
  const { hovering } = useFileDrop(addRefs, { enabled: active });
  const browse = async () => {
    const picked = await open({ multiple: true, filters: [{ name: "图片", extensions: IMAGE_EXTENSIONS }] });
    if (picked) addRefs(Array.isArray(picked) ? picked : [picked]);
  };

  const params = model === "gpt-image-2.6" ? gptParams : nanoParams;
  const setParams = (patch: Partial<GenParams>) =>
    model === "gpt-image-2.6" ? setGptParams((p) => ({ ...p, ...patch })) : setNanoParams((p) => ({ ...p, ...patch }));

  const hasKey = !!settings?.settings.apiKey;
  const refImages = mode === "image" ? refs : mode === "edit" && editSource ? editSource.files : [];
  const blocker = !hasKey
    ? "请先到「设置」填写 API Key"
    : !prompt.trim()
      ? "请填写提示词"
      : mode === "image" && !refs.length
        ? "请添加参考图"
        : mode === "edit" && !editSource
          ? "请先在队列或图库中选择要修改的图"
          : null;

  const submit = async () => {
    if (blocker) {
      toast.warning(blocker);
      return;
    }
    setSubmitting(true);
    try {
      const created = await api.enqueue({
        mode,
        model,
        prompt: prompt.trim(),
        params,
        refImages,
        count,
        parentId: mode === "edit" && editSource ? editSource.id : null,
      });
      log(`已加入队列：「${shortPrompt(prompt.trim())}」× ${created.length}`, "info");
      toast.success(`已加入队列 · ${created.length} 张`);
    } catch (err) {
      log(`加入队列失败：${errorText(err)}`, "error");
      toast.error(errorText(err));
    } finally {
      setSubmitting(false);
    }
  };

  const byId = useMemo(() => new Map(tasks.map((t) => [t.id, t])), [tasks]);
  const queue = useMemo(
    () => tasks.filter((t) => t.inQueue).sort((a, b) => b.createdAt - a.createdAt || a.seq - b.seq),
    [tasks]
  );
  const finishedCount = queue.filter((t) => !isActive(t.status)).length;
  const runningCount = queue.filter((t) => t.status === "generating" || t.status === "downloading").length;
  const queuedCount = queue.filter((t) => t.status === "queued").length;

  const run = async (label: string, fn: () => Promise<unknown>) => {
    try {
      await fn();
    } catch (err) {
      log(`${label}失败：${errorText(err)}`, "error");
      toast.error(errorText(err));
    }
  };

  const actions = {
    onEdit,
    onRetry: (t: Task) => run("重试", () => api.retry(t.id)),
    onRetryDownload: (t: Task) => run("重试下载", () => api.retryDownload(t.id)),
    onCancel: (t: Task) => run("取消", () => api.cancel(t.id)),
    onReveal: (t: Task) => run("打开文件夹", () => api.revealFile(t.files[0])),
    onDelete: (t: Task) =>
      modal.confirm({
        title: "删除这条任务记录？",
        content: isActive(t.status) ? "任务正在进行，会先取消再删除。" : "只删除记录，已下载的图片文件仍保留在输出目录。",
        okText: "删除",
        okButtonProps: { danger: true },
        onOk: () => run("删除", () => api.remove([t.id])),
      }),
  };

  const clearFinished = () =>
    modal.confirm({
      title: "清空已结束的任务？",
      content: "失败、已取消的记录会被删除；已完成的只从队列移除，仍保留在图库里。",
      okText: "清空",
      okButtonProps: { danger: true },
      onOk: () =>
        run("清空", async () => {
          const n = await api.clearFinished();
          log(`已清空 ${n} 个已结束的任务。`);
        }),
    });

  const submitHint = IS_MAC ? "⌘ + Enter 快速加入" : "Ctrl + Enter 快速加入";

  return (
    <Page>
      <Workspace
        aside={
          <>
            {/* 参数卡片内部滚动，主按钮常驻在右栏底部 */}
            <Panel fill title="出图参数" className="min-h-0 flex-1">
              <div className="-mr-2 min-h-0 flex-1 overflow-y-auto pr-2">
                <Form layout="vertical" component="div" className="flex flex-col gap-3 [&_.ant-form-item]:mb-0">
                  <Segmented block options={MODES} value={mode} onChange={(v) => setMode(v as Mode)} />

                  <Form.Item label="模型">
                    <Select
                      value={model}
                      onChange={setModel}
                      options={MODELS.map((m) => ({ value: m.value, label: m.label, title: m.hint }))}
                    />
                  </Form.Item>

                  <Form.Item label="提示词" extra={submitHint}>
                    <Input.TextArea
                      value={prompt}
                      onChange={(e) => setPrompt(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                          e.preventDefault();
                          submit();
                        }
                      }}
                      autoSize={{ minRows: 4, maxRows: 9 }}
                      placeholder={mode === "text" ? "描述你想要的画面，例如：清晨的江南水乡，水墨淡彩" : "描述要怎么改，例如：换成黄昏的光线，保留人物姿态"}
                      maxLength={4000}
                    />
                  </Form.Item>

                  {mode === "image" && (
                    <Form.Item label={`参考图 · ${refs.length} 张`}>
                      <div className="flex flex-col gap-2">
                        <DropZone
                          active={hovering}
                          compact={refs.length > 0}
                          onBrowse={browse}
                          title="拖入参考图，或点击选择"
                          hint="支持 JPG / PNG / WebP 等，长边超过 2048 会自动缩小"
                        />
                        <RefImages paths={refs} onRemove={(p) => setRefs((prev) => prev.filter((x) => x !== p))} />
                      </div>
                    </Form.Item>
                  )}

                  {mode === "edit" &&
                    (editSource ? (
                      <Form.Item label="修改来源">
                        <div className="flex items-center gap-2">
                          <Thumb path={editSource.files[0]} size="md" alt={editSource.prompt} />
                          <Typography.Paragraph type="secondary" ellipsis={{ rows: 3, tooltip: editSource.prompt }} className="m-0 min-w-0 flex-1 text-sm">
                            {editSource.prompt}
                          </Typography.Paragraph>
                        </div>
                      </Form.Item>
                    ) : (
                      <Alert type="info" showIcon title="在左侧队列或「图库」里，对已完成的图点「继续修改」。" />
                    ))}

                  {model === "gpt-image-2.6" ? (
                    <>
                      <Form.Item label="比例 / 尺寸">
                        <Select value={params.aspectRatio} onChange={(v) => setParams({ aspectRatio: v })} options={gptRatioOptions} />
                      </Form.Item>
                      <Form.Item label="背景">
                        <Segmented block options={BACKGROUNDS} value={params.background} onChange={(v) => setParams({ background: String(v) })} />
                      </Form.Item>
                      <Form.Item label="输出格式">
                        <Segmented
                          block
                          options={OUTPUT_FORMATS}
                          value={params.outputFormat || "png"}
                          onChange={(v) => setParams({ outputFormat: String(v) })}
                        />
                      </Form.Item>
                    </>
                  ) : (
                    <>
                      <Form.Item label="比例">
                        <Select value={params.aspectRatio} onChange={(v) => setParams({ aspectRatio: v })} options={nanoRatioOptions} />
                      </Form.Item>
                      <Form.Item label="分辨率">
                        <Segmented block options={IMAGE_SIZES} value={params.imageSize || "1K"} onChange={(v) => setParams({ imageSize: String(v) })} />
                      </Form.Item>
                    </>
                  )}

                  <Form.Item label="张数" extra="每张是一个独立任务，分别排队、分别计费">
                    <Segmented block options={COUNTS} value={count} onChange={(v) => setCount(Number(v))} />
                  </Form.Item>
                </Form>
              </div>
            </Panel>

            <div className="flex shrink-0 flex-col gap-2">
              {blocker && hasKey && <span className="text-sm text-text-tertiary">{blocker}</span>}
              <Button type="primary" block loading={submitting} disabled={!!blocker} onClick={submit}>
                加入队列 · {count} 张
              </Button>
            </div>
          </>
        }
      >
        <Panel
          fill
          className="flex-1"
          title={`任务队列 · 进行中 ${runningCount} · 排队 ${queuedCount}`}
          extra={
            <Button type="link" size="small" icon={<ClearOutlined />} disabled={!finishedCount} onClick={clearFinished}>
              清空已结束…
            </Button>
          }
        >
          {settings && !hasKey && (
            <Alert
              type="warning"
              showIcon
              title="还没有填写 API Key"
              description="出图需要 GRSAI 的 API Key，填好后可点「测试连接」验证（不扣费）。"
              action={
                <Button size="small" icon={<SettingOutlined />} onClick={onGoSettings}>
                  去设置
                </Button>
              }
            />
          )}
          <ListWell>
            {queue.length === 0 ? (
              <Empty
                image={<PictureOutlined className="text-h1 text-text-tertiary" />}
                description={hasKey ? "队列是空的。在右侧写好提示词，点「加入队列」开始出图" : "填写 API Key 后即可开始出图"}
                className="py-12"
              />
            ) : (
              <ul className="m-0 list-none p-0">
                {queue.map((t) => (
                  <TaskRow key={t.id} task={t} parent={t.parentId ? byId.get(t.parentId) : undefined} {...actions} />
                ))}
              </ul>
            )}
          </ListWell>
        </Panel>
      </Workspace>

      <Panel
        fill
        title="出图日志"
        size="small"
        className="h-[170px] shrink-0"
        extra={
          <Button type="link" size="small" disabled={!logs.length} onClick={clear}>
            清空日志
          </Button>
        }
      >
        <LogViewer logs={logs} emptyText="加入队列后，生成与下载过程会显示在这里" />
      </Panel>
    </Page>
  );
}
