import { useEffect, useMemo, useRef, useState } from "react";
import { AppstoreOutlined, CopyOutlined, DeleteOutlined, EyeOutlined, FolderOpenOutlined, PictureOutlined } from "@ant-design/icons";
import { App as AntApp, Button, Descriptions, Segmented, Typography } from "antd";
import { EmptyState, ImageGrid, ListWell, LogViewer, Page, Panel, Thumb, Workspace, fileName, useLogger } from "../ui-kit";
import { api, errorText } from "../lib/api";
import { formatTime, modeLabel, modelLabel, paramsSummary, shortPrompt, taskNo } from "../lib/options";
import type { ModelId, Task } from "../lib/types";

interface GalleryItem {
  key: string;
  task: Task;
  file: string;
}

type Filter = "all" | ModelId;
const FILTERS = [
  { value: "all", label: "全部" },
  { value: "nano-banana-2.1", label: "Nano" },
  { value: "gpt-image-2.6", label: "GPT" },
];

interface GalleryPageProps {
  tasks: Task[];
  onEdit: (task: Task, file?: string) => void;
  /** 从队列「改自」跳过来时要定位的图 */
  focus: { taskId: string; file?: string; nonce: number } | null;
}

export function GalleryPage({ tasks, onEdit, focus }: GalleryPageProps) {
  const { message, modal } = AntApp.useApp();
  const { logs, log, clear } = useLogger();
  const [filter, setFilter] = useState<Filter>("all");
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [previewIndex, setPreviewIndex] = useState<number | null>(null);
  const gridRef = useRef<HTMLDivElement>(null);

  const items = useMemo<GalleryItem[]>(
    () =>
      tasks
        .filter((t) => t.status === "succeeded" && (filter === "all" || t.model === filter))
        .sort((a, b) => (b.finishedAt ?? b.createdAt) - (a.finishedAt ?? a.createdAt))
        .flatMap((task) => task.files.map((file) => ({ key: file, task, file }))),
    [tasks, filter]
  );
  const byId = useMemo(() => new Map(tasks.map((t) => [t.id, t])), [tasks]);
  const selected = items.find((i) => i.key === selectedKey) ?? items[0] ?? null;

  // 定位某条记录（某一张）：清掉筛选、选中并滚到可见
  const locate = (taskId: string, file?: string) => {
    const task = tasks.find((t) => t.id === taskId);
    const key = file ?? task?.files[0];
    if (!task || !key) return false;
    setFilter("all");
    setSelectedKey(key);
    // ImageGrid 不会把选中项滚到可见，这里按选中态找到瓦片再滚动
    window.setTimeout(
      () => gridRef.current?.querySelector('[aria-pressed="true"]')?.scrollIntoView({ behavior: "smooth", block: "center" }),
      60
    );
    return true;
  };
  useEffect(() => {
    if (focus) locate(focus.taskId, focus.file);
    // 只在收到新的定位请求时执行
  }, [focus]);

  // 选中项被删除后回到第一张
  useEffect(() => {
    if (selectedKey && !items.some((i) => i.key === selectedKey)) setSelectedKey(null);
  }, [items, selectedKey]);

  const openPreview = (key: string) => {
    const index = items.findIndex((i) => i.key === key);
    if (index >= 0) setPreviewIndex(index);
  };

  const reveal = async (item: GalleryItem) => {
    try {
      await api.revealFile(item.file);
      log(`已在文件管理器中显示：${fileName(item.file)}`);
    } catch (err) {
      log(`打开文件夹失败：${errorText(err)}`, "error");
      message.error(errorText(err));
    }
  };

  const copyPrompt = async (task: Task) => {
    try {
      await navigator.clipboard.writeText(task.prompt);
      message.success("已复制提示词");
      log(`已复制提示词：「${shortPrompt(task.prompt, 30)}」`, "success");
    } catch (err) {
      message.error(`复制失败：${errorText(err)}`);
    }
  };

  const remove = (task: Task) =>
    modal.confirm({
      title: "从图库删除这条记录？",
      content: "只删除记录，图片文件仍保留在输出目录。",
      okText: "删除",
      okButtonProps: { danger: true },
      onOk: async () => {
        try {
          await api.remove([task.id]);
          log(`已删除记录：「${shortPrompt(task.prompt)}」`, "warning");
        } catch (err) {
          message.error(errorText(err));
        }
      },
    });

  const parent = selected?.task.parentId ? byId.get(selected.task.parentId) : undefined;

  return (
    <Page>
      <Workspace
        aside={
          <>
            <Panel fill title="图片信息" className="min-h-0 flex-1">
              {selected ? (
                <div className="-mr-2 flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto pr-2">
                  <Thumb path={selected.file} size="fill" fit="contain" alt={selected.task.prompt} preview={false} />
                  <Typography.Paragraph ellipsis={{ rows: 4, expandable: "collapsible" }} className="m-0">
                    {selected.task.prompt}
                  </Typography.Paragraph>
                  <Descriptions
                    size="small"
                    column={1}
                    items={[
                      { key: "no", label: "编号", children: taskNo(selected.task, selected.file) },
                      { key: "model", label: "模型", children: modelLabel(selected.task.model) },
                      { key: "mode", label: "模式", children: modeLabel(selected.task.mode) },
                      ...(selected.task.count > 1
                        ? [{ key: "part", label: "多合一", children: `第 ${selected.task.files.indexOf(selected.file) + 1} / ${selected.task.files.length} 张` }]
                        : []),
                      { key: "params", label: "参数", children: paramsSummary(selected.task.model, selected.task.params) },
                      ...(selected.task.parentId
                        ? [
                            {
                              key: "parent",
                              label: "改自",
                              children: parent ? (
                                <Button
                                  type="link"
                                  size="small"
                                  className="h-auto p-0"
                                  onClick={() => locate(parent.id, selected.task.parentPart ? parent.files[selected.task.parentPart - 1] : undefined)}
                                >
                                  #{parent.number}
                                  {selected.task.parentPart ? `-${selected.task.parentPart}` : ""}「{shortPrompt(parent.prompt, 8)}」
                                </Button>
                              ) : (
                                <span className="text-text-tertiary">
                                  {selected.task.parentNumber ? `#${selected.task.parentNumber}` : ""}（原记录已删除）
                                </span>
                              ),
                            },
                          ]
                        : []),
                      { key: "time", label: "完成", children: formatTime(selected.task.finishedAt ?? selected.task.createdAt) },
                      {
                        key: "file",
                        label: "文件",
                        children: (
                          <Typography.Text ellipsis={{ tooltip: selected.file }} className="max-w-[190px]">
                            {fileName(selected.file)}
                          </Typography.Text>
                        ),
                      },
                    ]}
                  />
                  <div className="grid grid-cols-2 gap-2">
                    <Button icon={<EyeOutlined />} onClick={() => openPreview(selected.key)}>
                      大图预览
                    </Button>
                    <Button icon={<CopyOutlined />} onClick={() => copyPrompt(selected.task)}>
                      复制提示词
                    </Button>
                    <Button icon={<FolderOpenOutlined />} onClick={() => reveal(selected)}>
                      打开文件夹
                    </Button>
                    <Button danger icon={<DeleteOutlined />} onClick={() => remove(selected.task)}>
                      删除记录…
                    </Button>
                    {selected.task.compositeFile && (
                      <Button icon={<AppstoreOutlined />} className="col-span-2" onClick={() => reveal({ ...selected, file: selected.task.compositeFile! })}>
                        查看合成原图
                      </Button>
                    )}
                  </div>
                </div>
              ) : (
                <EmptyState size="small" icon={<PictureOutlined />} description="选择一张图片查看详情" />
              )}
            </Panel>
          </>
        }
        asideFooter={
          <Button type="primary" block disabled={!selected} onClick={() => selected && onEdit(selected.task, selected.file)}>
            继续修改 · 这张
          </Button>
        }
      >
        <Panel
          fill
          className="flex-1"
          title={`图库 · ${items.length} 张`}
          extra={<Segmented size="small" options={FILTERS} value={filter} onChange={(v) => setFilter(v as Filter)} />}
        >
          <ListWell ref={gridRef} className="p-3">
            {items.length === 0 ? (
              <EmptyState
                icon={<PictureOutlined />}
                title={filter === "all" ? "还没有已完成的图片" : "这个模型还没有已完成的图片"}
                description="出图完成后会出现在这里；多合一拆切后的每一张都会单独列出"
              />
            ) : (
              <ImageGrid
                items={items.map((i) => ({
                  key: i.key,
                  path: i.file,
                  alt: `${taskNo(i.task, i.file)} ${i.task.prompt}`,
                  badge: <span className="rounded-sm border bg-container px-1 text-sm tabular-nums text-text-secondary">{taskNo(i.task, i.file)}</span>,
                }))}
                selectedKey={selected?.key}
                onSelect={setSelectedKey}
                previewIndex={previewIndex}
                onPreviewIndexChange={setPreviewIndex}
              />
            )}
          </ListWell>
        </Panel>
      </Workspace>

      <Panel fill title="图库日志" size="small" className="h-[170px] shrink-0">
        <LogViewer logs={logs} onClear={clear} emptyText="在这里查看复制、打开文件夹等操作记录" />
      </Panel>
    </Page>
  );
}
