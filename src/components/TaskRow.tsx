import { useState } from "react";
import {
  AppstoreOutlined,
  BranchesOutlined,
  CloudDownloadOutlined,
  DeleteOutlined,
  EditOutlined,
  EyeOutlined,
  FolderOpenOutlined,
  MoreOutlined,
  RedoOutlined,
  StopOutlined,
} from "@ant-design/icons";
import { Button, Dropdown, Progress, Tag, Tooltip, Typography, type MenuProps } from "antd";
import { LinkTag, ListRow, StatusTag, Thumb, ThumbStrip } from "../ui-kit";
import { bytesText, formatTime, isActive, modeLabel, modelLabel, paramsSummary, shortPrompt } from "../lib/options";
import type { Task } from "../lib/types";

export interface TaskActions {
  /** file：作为参考图的那一张（多合一时是拆切后的某一张） */
  onEdit: (task: Task, file?: string) => void;
  onRetry: (task: Task) => void;
  onRetryDownload: (task: Task) => void;
  onCancel: (task: Task) => void;
  onDelete: (task: Task) => void;
  onReveal: (task: Task) => void;
}

interface TaskRowProps extends TaskActions {
  task: Task;
  parent?: Task;
  /** 被「改自」跳转定位时高亮（kit useRowFlash + ListRow highlight：主色内描边并滚到可见） */
  highlighted?: boolean;
  /** 点击「改自 #7」：定位到原记录 */
  onJumpParent?: (task: Task) => void;
}

/** 「下载中 45% · 1.5 / 3.3 MB」；还没拿到总大小时只写已下载 */
function downloadText(task: Task) {
  if (!task.downloadBytes) return "下载中";
  if (task.downloadTotal) {
    const pct = Math.floor((task.downloadBytes / task.downloadTotal) * 100);
    return `下载中 ${pct}% · ${bytesText(task.downloadBytes)} / ${bytesText(task.downloadTotal)}`;
  }
  return `下载中 · ${bytesText(task.downloadBytes)}`;
}

/** 绘境任务状态 → kit StatusTag */
function TaskStatus({ task }: { task: Task }) {
  switch (task.status) {
    case "queued":
      return <StatusTag status="queued" />;
    case "generating":
      return <StatusTag status="processing">{task.remoteId ? `生成中 ${task.progress}%` : "生成中"}</StatusTag>;
    case "downloading":
      return <StatusTag status="processing">{downloadText(task)}</StatusTag>;
    case "succeeded":
      return <StatusTag status="success" />;
    case "failed":
      return <StatusTag status="error" />;
    case "download_failed":
      return <StatusTag status="warning">下载失败</StatusTag>;
    case "cancelled":
      return <StatusTag status="cancelled" />;
  }
}

/**
 * 队列行：kit ListRow（缩略图 + 序号与提示词 + 状态 / 模型 / 模式 / 参数 + 进度 / 报错 / 拆切图 + 行尾操作）。
 * 行尾按 kit 约定最多 3 个图标按钮，其余收进「更多」，删除放最后。
 */
export function TaskRow({ task, parent, highlighted, onJumpParent, onEdit, onRetry, onRetryDownload, onCancel, onDelete, onReveal }: TaskRowProps) {
  const [previewOpen, setPreviewOpen] = useState(false);
  const failed = task.status === "failed" || task.status === "download_failed";
  const done = task.status === "succeeded";
  const active = isActive(task.status);
  const file = task.files[0];
  const parts = task.files.length > 1 ? task.files : [];
  // 左侧大缩略图：多合一时显示合成原图，下面一排是拆切后的各张
  const cover = task.compositeFile ?? file;

  const more: NonNullable<MenuProps["items"]> = [
    ...(done && file ? [{ key: "reveal", icon: <FolderOpenOutlined />, label: "打开所在文件夹" }] : []),
    ...(task.status === "download_failed" ? [{ key: "download", icon: <CloudDownloadOutlined />, label: "仅重试下载（不扣费）" }] : []),
    ...(task.status === "download_failed" ? [{ key: "retry", icon: <RedoOutlined />, label: "重新生成（会再次扣费）" }] : []),
    { type: "divider" as const },
    { key: "delete", icon: <DeleteOutlined />, label: "删除记录…", danger: true },
  ];
  const onMore: MenuProps["onClick"] = ({ key }) => {
    if (key === "reveal") onReveal(task);
    else if (key === "download") onRetryDownload(task);
    else if (key === "retry") onRetry(task);
    else if (key === "delete") onDelete(task);
  };

  return (
    <ListRow
      id={`task-row-${task.id}`}
      status={failed ? "error" : undefined}
      failedLabel={false}
      highlight={highlighted}
      leading={
        <Thumb
          path={cover}
          size="md"
          fit="contain"
          loading={task.status === "generating" || task.status === "downloading"}
          open={previewOpen}
          onOpenChange={setPreviewOpen}
          alt={task.prompt}
        />
      }
      title={
        <span className="flex min-w-0 items-baseline gap-2">
          <span className="shrink-0 font-strong tabular-nums text-primary-text">#{task.number}</span>
          <Typography.Text strong ellipsis={{ tooltip: task.prompt }} className="min-w-0">
            {task.prompt}
          </Typography.Text>
        </span>
      }
      meta={
        <>
          <TaskStatus task={task} />
          <Tag variant="filled">{modelLabel(task.model)}</Tag>
          <Tag variant="filled">{modeLabel(task.mode)}</Tag>
          {task.count > 1 && (
            <Tooltip
              title={
                task.grid
                  ? `整张 ${task.grid.requestRatio === "auto" ? "自动比例" : task.grid.requestRatio}，${task.grid.cols} 列 × ${task.grid.rows} 行等分${task.grid.cellRatio ? `，每张约 ${task.grid.cellRatio}` : ""}，1 次请求`
                  : undefined
              }
            >
              <Tag icon={<AppstoreOutlined />} variant="outlined">
                {task.count} 合 1
              </Tag>
            </Tooltip>
          )}
          {task.parentId && <ParentTag task={task} parent={parent} onJump={onJumpParent} />}
          <span>
            {paramsSummary(task.model, task.params)} · {formatTime(task.createdAt)}
          </span>
        </>
      }
      extra={
        <>
          {task.status === "generating" && task.remoteId && (
            <Progress percent={task.progress} size="small" showInfo={false} status="active" className="m-0 max-w-[320px]" />
          )}
          {task.status === "downloading" && task.downloadTotal ? (
            <Progress
              percent={Math.floor((task.downloadBytes / task.downloadTotal) * 100)}
              size="small"
              showInfo={false}
              status="active"
              className="m-0 max-w-[320px]"
            />
          ) : null}
          {task.error && <Typography.Text className="text-sm">{task.error}</Typography.Text>}
          {task.notice && <Typography.Text type="warning" className="text-sm">{task.notice}</Typography.Text>}
          {parts.length > 0 && (
            <ThumbStrip
              // 长条切片完整显示：ThumbStrip 没有 fit 参数，从外层把 img 改成 contain
              className="[&_img]:object-contain"
              items={parts.map((p, i) => ({ path: p, caption: `#${task.number}-${i + 1}`, alt: `#${task.number}-${i + 1}` }))}
            />
          )}
        </>
      }
      trailing={
        <>
          {done && file && (
            <Tooltip title="预览">
              <Button type="text" size="small" icon={<EyeOutlined />} aria-label="预览" onClick={() => setPreviewOpen(true)} />
            </Tooltip>
          )}
          {done &&
            file &&
            (parts.length > 0 ? (
              <Dropdown
                trigger={["click"]}
                menu={{ items: parts.map((p, i) => ({ key: p, label: `继续修改 #${task.number}-${i + 1}` })), onClick: ({ key }) => onEdit(task, key) }}
              >
                <Tooltip title="继续修改（选一张）">
                  <Button type="text" size="small" icon={<EditOutlined />} aria-label="继续修改" />
                </Tooltip>
              </Dropdown>
            ) : (
              <Tooltip title="继续修改">
                <Button type="text" size="small" icon={<EditOutlined />} aria-label="继续修改" onClick={() => onEdit(task)} />
              </Tooltip>
            ))}
          {(task.status === "failed" || task.status === "cancelled") && (
            <Tooltip title="重试">
              <Button type="text" size="small" icon={<RedoOutlined />} aria-label="重试" onClick={() => onRetry(task)} />
            </Tooltip>
          )}
          {task.status === "download_failed" && (
            <Tooltip title="仅重试下载（不重新生成，不扣费）">
              <Button type="text" size="small" icon={<CloudDownloadOutlined />} aria-label="重试下载" onClick={() => onRetryDownload(task)} />
            </Tooltip>
          )}
          {active && (
            <Tooltip title={task.status === "queued" ? "取消排队" : "取消（丢弃结果）"}>
              <Button type="text" size="small" icon={<StopOutlined />} aria-label="取消" onClick={() => onCancel(task)} />
            </Tooltip>
          )}
          <Dropdown trigger={["click"]} menu={{ items: more, onClick: onMore }}>
            <Tooltip title="更多">
              <Button type="text" size="small" icon={<MoreOutlined />} aria-label="更多操作" />
            </Tooltip>
          </Dropdown>
        </>
      }
    />
  );
}

/** 「改自 #7」/「改自 #7-2」：kit LinkTag，点击跳到原记录；原记录已删除时 disabled 置灰 */
function ParentTag({ task, parent, onJump }: { task: Task; parent?: Task; onJump?: (task: Task) => void }) {
  const number = parent?.number ?? task.parentNumber;
  const label = number ? `改自 #${number}${task.parentPart ? `-${task.parentPart}` : ""}` : "改自已删除的记录";
  return (
    <Tooltip title={parent ? `${parent.inQueue ? "定位到" : "在图库中查看"}原记录：${shortPrompt(parent.prompt, 24)}` : "原记录已删除"}>
      <LinkTag icon={<BranchesOutlined />} disabled={!parent} onClick={() => onJump?.(task)}>
        {label}
      </LinkTag>
    </Tooltip>
  );
}
