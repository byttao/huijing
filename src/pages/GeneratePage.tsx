import { useEffect, useMemo, useRef, useState } from "react";
import { ClearOutlined, DownOutlined, KeyOutlined, PictureOutlined, SettingOutlined } from "@ant-design/icons";
import { Alert, App as AntApp, Button, Checkbox, Form, Input, Segmented, Select, Typography } from "antd";
import { open } from "@tauri-apps/plugin-dialog";
import {
  AdvancedSection,
  DropZone,
  EmptyState,
  FieldHint,
  IMAGE_EXTENSIONS,
  ListWell,
  LogActions,
  LogViewer,
  Page,
  Panel,
  RatioPreview,
  RatioSelect,
  SettingsScroll,
  Thumb,
  ThumbStrip,
  Workspace,
  fileName,
  useFileDrop,
  useLogger,
  useRowFlash,
} from "../ui-kit";
import { TaskRow } from "../components/TaskRow";
import { api, errorText } from "../lib/api";
import { BACKGROUNDS, GPT_RATIO_OPTIONS, IMAGE_SIZES, MODELS, MODES, NANO_RATIOS, OUTPUT_FORMATS, isActive, shortPrompt, sizeText, taskNo } from "../lib/options";
import type { EditRequest, GenParams, GridPlan, Mode, ModelId, SettingsView, Task } from "../lib/types";

const IS_MAC = /Mac/i.test(navigator.userAgent);
const COUNTS = [1, 2, 3, 4].map((n) => ({ value: n, label: n === 1 ? "1 张" : `${n} 合 1` }));
const MAX_REFS = 8;

interface GeneratePageProps {
  active: boolean;
  tasks: Task[];
  tasksLoaded: boolean;
  settings: SettingsView | null;
  editRequest: EditRequest | null;
  onEdit: (task: Task, file?: string) => void;
  onGoSettings: () => void;
  /** 原记录已从队列清除（仍在图库）时，跳到图库定位 */
  onShowInGallery: (task: Task, file?: string) => void;
}

export function GeneratePage({ active, tasks, tasksLoaded, settings, editRequest, onEdit, onGoSettings, onShowInGallery }: GeneratePageProps) {
  const { message, modal } = AntApp.useApp();
  // 日志通道 generate：Rust 端的 queue-log 在 main.tsx 里写进来，页面挂载前的先缓冲
  const { logs, log, clear } = useLogger({ channel: "generate" });

  const [mode, setMode] = useState<Mode>("text");
  const [model, setModel] = useState<ModelId>("nano-banana-2.1");
  const [prompt, setPrompt] = useState("");
  const [gptParams, setGptParams] = useState<GenParams>({ aspectRatio: "1:1", imageSize: "1K", background: "", outputFormat: "png", higherRes: false, layout: "" });
  const [nanoParams, setNanoParams] = useState<GenParams>({ aspectRatio: "auto", imageSize: "1K", background: "", outputFormat: "png", higherRes: false, layout: "" });
  const [count, setCount] = useState(1);
  const [refs, setRefs] = useState<string[]>([]);
  const [editSource, setEditSource] = useState<{ task: Task; file: string } | null>(null);
  const [plan, setPlan] = useState<GridPlan | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  // 选了张数 / 排法后把宫格预览滚到可见（小窗口下预览在折叠线以下）
  const previewRef = useRef<HTMLDivElement>(null);
  const revealPreview = () => window.setTimeout(() => previewRef.current?.scrollIntoView({ block: "nearest", behavior: "smooth" }), 80);
  // 参数卡片标题栏的「比例与张数 ↓」：用 SettingsScroll 的 ref 把比例那一项滚到顶部
  // （「下面还有」的滚动提示由 SettingsScroll 自带）。antd Form.Item 不把 id 落到 DOM，id 给控件
  const paramsScrollRef = useRef<HTMLDivElement>(null);
  const jumpToSize = () =>
    paramsScrollRef.current?.querySelector("#gen-ratio")?.closest(".ant-form-item")?.scrollIntoView({ behavior: "smooth", block: "start" });
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
    const { task, file } = editRequest;
    setEditSource({ task, file });
    setMode("edit");
    // 改的是其中一张：张数回到 1 张，其余沿用原任务参数
    setCount(1);
    setModel(task.model);
    if (task.model === "gpt-image-2.6") setGptParams(task.params);
    else setNanoParams(task.params);
    setPrompt(task.prompt);
    log(`继续修改 ${taskNo(task, file)}「${shortPrompt(task.prompt)}」：已带入这张图与原参数（张数重置为 1 张），改好提示词后加入队列。`);
  }, [editRequest, log]);

  // 首次载入时记一条摘要（恢复中断任务的详情由 Rust 端日志补上）
  const announced = useRef(false);
  useEffect(() => {
    if (!tasksLoaded || announced.current) return;
    announced.current = true;
    if (tasks.length) log(`已载入 ${tasks.length} 条任务记录。`);
  }, [tasksLoaded, tasks, log]);

  const addRefs = (paths: string[]) => {
    const fresh = paths.filter((p) => !refs.includes(p));
    if (!fresh.length) return;
    const room = MAX_REFS - refs.length;
    if (room <= 0) {
      message.warning(`参考图最多 ${MAX_REFS} 张`);
      return;
    }
    const taken = fresh.slice(0, room);
    setRefs((prev) => [...prev, ...taken]);
    if (mode !== "image") setMode("image");
    log(`已添加 ${taken.length} 张参考图：${taken.map(fileName).join("、")}`);
    if (taken.length < fresh.length) message.warning(`参考图最多 ${MAX_REFS} 张，多出的已忽略`);
  };
  const { hovering } = useFileDrop(addRefs, { enabled: active });
  const browse = async () => {
    const picked = await open({ multiple: true, filters: [{ name: "图片", extensions: IMAGE_EXTENSIONS }] });
    if (picked) addRefs(Array.isArray(picked) ? picked : [picked]);
  };

  const params = model === "gpt-image-2.6" ? gptParams : nanoParams;
  const setParams = (patch: Partial<GenParams>) =>
    model === "gpt-image-2.6" ? setGptParams((p) => ({ ...p, ...patch })) : setNanoParams((p) => ({ ...p, ...patch }));

  // 宫格方案预览：与 Rust 端加入队列时同一套计算；设置里的「放大」开关变了也要重算
  const upscaleSetting = settings?.settings.upscaleParts;
  useEffect(() => {
    let stale = false;
    api
      .planGrid(model, params, count)
      .then((p) => !stale && (setPlan(p), setPlanError(null)))
      .catch((err) => !stale && (setPlan(null), setPlanError(errorText(err))));
    return () => {
      stale = true;
    };
  }, [model, params, count, upscaleSetting]);

  // GPT 选了 GIF / SVG 时不能多合一（无法拆切）；比例为自动时可以，每张比例取决于实际出图
  const multiBlocked =
    model === "gpt-image-2.6" && (params.outputFormat === "gif" || params.outputFormat === "svg")
      ? "GIF / SVG 输出无法拆切，多合一请用 PNG"
      : null;
  useEffect(() => {
    if (multiBlocked && count > 1) setCount(1);
  }, [multiBlocked, count]);

  // 「高级」里改过几项（相对默认值），以及一键恢复默认
  const advancedChanged =
    model === "gpt-image-2.6" ? Number(params.background !== "") + Number((params.outputFormat || "png") !== "png") : Number(params.higherRes);
  const resetAdvanced = () => setParams(model === "gpt-image-2.6" ? { background: "", outputFormat: "png" } : { higherRes: false });

  const hasKey = !!settings?.settings.apiKey;
  const refImages = mode === "image" ? refs : mode === "edit" && editSource ? [editSource.file] : [];
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
      message.warning(blocker);
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
        parentId: mode === "edit" && editSource ? editSource.task.id : null,
      });
      const t = created[0];
      log(`已加入队列：#${t?.number} 「${shortPrompt(prompt.trim())}」${count > 1 ? `· ${count} 合 1（${t?.grid?.cols}×${t?.grid?.rows} 宫格，切片为 #${t?.number}-1 …）` : ""}`, "info");
      message.success(count > 1 ? `已加入队列 · ${count} 合 1（1 次请求）` : "已加入队列 · 1 张");
    } catch (err) {
      log(`加入队列失败：${errorText(err)}`, "error");
      message.error(errorText(err));
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
      message.error(errorText(err));
    }
  };

  // 「改自 #7」：滚动到原记录并高亮闪一下；原记录已移出队列时去图库定位
  // kit useRowFlash：定位后高亮 2.5 秒自动取消（ListRow 的 highlight 负责滚到可见）
  const { flashKey, flash } = useRowFlash(2500);
  const jumpToParent = (task: Task) => {
    const parent = task.parentId ? byId.get(task.parentId) : undefined;
    if (!parent) return;
    const parentFile = task.parentPart ? parent.files[task.parentPart - 1] : undefined;
    if (!parent.inQueue) {
      onShowInGallery(parent, parentFile);
      return;
    }
    // kit ListRow 的 highlight：主色内描边并滚到可见；保持 2.5 秒后取消
    flash(parent.id);
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
        asideFooter={
          <>
            {blocker && hasKey && <FieldHint>{blocker}</FieldHint>}
            <Button type="primary" block loading={submitting} disabled={!!blocker} onClick={submit}>
              {count > 1 ? `加入队列 · ${count} 合 1` : "加入队列 · 1 张"}
            </Button>
          </>
        }
        aside={
          <>
            {/* 参数卡片内部滚动；主按钮在 asideFooter 常驻 */}
            <Panel
              fill
              title="出图参数"
              className="min-h-0 flex-1"
              extra={
                <Button type="link" size="small" icon={<DownOutlined />} iconPlacement="end" onClick={jumpToSize}>
                  比例与张数
                </Button>
              }
            >
              <SettingsScroll ref={paramsScrollRef}>
                <Form layout="vertical" component="div" className="flex flex-col gap-3">
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
                      allowClear
                      autoSize={{ minRows: 3, maxRows: 8 }}
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
                        <ThumbStrip items={refs} columns={4} showName onRemove={(p) => setRefs((prev) => prev.filter((x) => x !== p))} />
                      </div>
                    </Form.Item>
                  )}

                  {mode === "edit" &&
                    (editSource ? (
                      <Form.Item label="修改来源">
                        <div className="flex items-center gap-2">
                          <Thumb path={editSource.file} size="md" fit="contain" alt={editSource.task.prompt} />
                          <Typography.Paragraph type="secondary" ellipsis={{ rows: 3, tooltip: editSource.task.prompt }} className="m-0 min-w-0 flex-1 text-sm">
                            {taskNo(editSource.task, editSource.file)} · {editSource.task.prompt}
                          </Typography.Paragraph>
                        </div>
                      </Form.Item>
                    ) : (
                      <FieldHint>在左侧队列或「图库」里，对已完成的图点「继续修改」。</FieldHint>
                    ))}

                  {/* 比例、张数、排法与宫格预览放在一起 */}
                  {model === "gpt-image-2.6" ? (
                    <Form.Item label="比例 / 尺寸">
                      <RatioSelect id="gen-ratio" value={params.aspectRatio} onChange={(v) => setParams({ aspectRatio: v })} options={GPT_RATIO_OPTIONS} />
                    </Form.Item>
                  ) : (
                    <div className="grid grid-cols-[minmax(0,1fr)_auto] gap-2">
                      <Form.Item label="比例">
                        <RatioSelect id="gen-ratio" value={params.aspectRatio} onChange={(v) => setParams({ aspectRatio: v })} options={NANO_RATIOS} />
                      </Form.Item>
                      <Form.Item label="分辨率">
                        <Segmented options={IMAGE_SIZES} value={params.imageSize || "1K"} onChange={(v) => setParams({ imageSize: String(v) })} />
                      </Form.Item>
                    </div>
                  )}

                  <Form.Item
                    label="张数"
                    extra={
                      multiBlocked ? (
                        <FieldHint type="warning">{multiBlocked}</FieldHint>
                      ) : count > 1 ? (
                        `一次请求画 ${count} 个方案、只扣一次费，按宫格等分拆成 ${count} 张`
                      ) : (
                        "选 2–4 张时在一张图里按宫格画多个方案，再等分拆切"
                      )
                    }
                  >
                    <Segmented
                      block
                      options={COUNTS}
                      value={count}
                      disabled={!!multiBlocked}
                      onChange={(v) => {
                        setCount(Number(v));
                        revealPreview();
                      }}
                    />
                  </Form.Item>

                  {(count === 2 || count === 3) && (
                    <Form.Item label="排法">
                      <Segmented
                        block
                        options={[
                          { value: "row", label: `横排 ${count}×1` },
                          { value: "column", label: `竖排 1×${count}` },
                        ]}
                        value={plan?.orientation || "row"}
                        onChange={(v) => {
                          setParams({ layout: String(v) });
                          revealPreview();
                        }}
                      />
                    </Form.Item>
                  )}

                  <div ref={previewRef}>
                    <Form.Item label={count > 1 ? "预览 · 宫格与尺寸" : "预览"}>
                      {plan ? (
                        <RatioPreview
                          ratio={plan.requestRatio}
                          cols={plan.cols}
                          rows={plan.rows}
                          height={80}
                          autoText={
                            plan.count > 1
                              ? `比例为「自动」：由模型决定整张图的比例，按 ${plan.cols} 列 × ${plan.rows} 行等分拆切，每张比例取决于实际出图`
                              : "比例为「自动」：由模型决定画面比例，尺寸出图后才知道"
                          }
                          caption={<PlanCaption plan={plan} />}
                        />
                      ) : (
                        planError && <FieldHint type="warning">{planError}</FieldHint>
                      )}
                    </Form.Item>
                  </div>

                  {/* kit AdvancedSection：低频参数默认折叠，标题旁显示「已改 N 项」，右侧「恢复默认」 */}
                  <AdvancedSection
                    // 300px 右栏里标题 +「已改 N 项」+「恢复默认」放不下长标题，只写「高级」
                    title="高级"
                    changedCount={advancedChanged}
                    extra={
                      advancedChanged > 0 && (
                        <Button type="link" size="small" onClick={resetAdvanced}>
                          恢复默认
                        </Button>
                      )
                    }
                  >
                    {model === "gpt-image-2.6" ? (
                      <>
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
                      <Form.Item
                        extra={
                          count === 1 ? (
                            <FieldHint>只在多合一（2–4 张）时生效</FieldHint>
                          ) : params.imageSize === "4K" ? (
                            <FieldHint>已是最高档 4K，无法再升档</FieldHint>
                          ) : (
                            <FieldHint type="warning">选 1K 时用 2K 请求，切出来每张像素更多、少靠放大（4 合 1 时正好是原生 1K）。高档位单价可能更高</FieldHint>
                          )
                        }
                      >
                        <Checkbox
                          checked={params.higherRes}
                          disabled={params.imageSize === "4K" || count === 1}
                          onChange={(e) => setParams({ higherRes: e.target.checked })}
                        >
                          合图使用更高一档分辨率
                        </Checkbox>
                      </Form.Item>
                    )}
                  </AdvancedSection>
                </Form>
              </SettingsScroll>
            </Panel>
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
          {settings && !hasKey && queue.length > 0 && (
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
              settings && !hasKey ? (
                <EmptyState
                  icon={<KeyOutlined />}
                  title="还没有填写 API Key"
                  description="出图需要 GRSAI 的 API Key，填好后可点「测试连接」验证（不扣费）。"
                  action={
                    <Button icon={<SettingOutlined />} onClick={onGoSettings}>
                      去设置
                    </Button>
                  }
                />
              ) : (
                <EmptyState icon={<PictureOutlined />} title="队列是空的" description="在右侧写好提示词，点「加入队列」开始出图" />
              )
            ) : (
              <ul className="m-0 list-none p-0">
                {queue.map((t) => (
                  <TaskRow
                    key={t.id}
                    task={t}
                    parent={t.parentId ? byId.get(t.parentId) : undefined}
                    highlighted={flashKey === t.id}
                    onJumpParent={jumpToParent}
                    {...actions}
                  />
                ))}
              </ul>
            )}
          </ListWell>
        </Panel>
      </Workspace>

      <Panel fill title="出图日志" size="small" className="h-[170px] shrink-0" extra={<LogActions logs={logs} onClear={clear} />}>
        <LogViewer logs={logs} emptyText="加入队列后，生成与下载过程会显示在这里" />
      </Panel>
    </Page>
  );
}

/** 预览框下的尺寸说明：整张尺寸；多合一时每张比例与尺寸，以及是否放大 */
function PlanCaption({ plan }: { plan: GridPlan }) {
  return (
    <span className="flex flex-col">
      <span>
        整张 {plan.requestRatio}
        {plan.requestTier ? ` · ${plan.requestTier}` : ""} · 约 {sizeText(plan.requestSize)}
        {plan.count > 1 && ` · ${plan.cols} 列 × ${plan.rows} 行`}
      </span>
      {plan.count > 1 && (
        <span className="text-text-secondary">
          拆切后每张约 {plan.cellRatio}（约 {sizeText(plan.cellSize)}）
          {plan.upscale &&
            (plan.outputSize[0] !== plan.cellSize[0] || plan.outputSize[1] !== plan.cellSize[1]
              ? `，放大到约 ${sizeText(plan.outputSize)}`
              : "，长边已等于整张图长边，不再放大")}
        </span>
      )}
    </span>
  );
}
