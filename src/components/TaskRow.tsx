import { useState } from "react";
import {
  AppstoreOutlined,
  BranchesOutlined,
  CloudDownloadOutlined,
  DeleteOutlined,
  EditOutlined,
  EyeOutlined,
  FolderOpenOutlined,
  RedoOutlined,
  StopOutlined,
} from "@ant-design/icons";
import { Button, Dropdown, Image, Progress, Tag, Tooltip, Typography } from "antd";
import { FILE_ROW_ERROR_CLASS, cn } from "../ui-kit";
import { formatTime, isActive, modeLabel, modelLabel, paramsSummary, shortPrompt } from "../lib/options";
import type { Task } from "../lib/types";
import { StatusTag } from "./StatusTag";
import { Thumb } from "./Thumb";

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
  /** 被「改自」跳转定位时高亮闪一下 */
  highlighted?: boolean;
  /** 点击「改自 #7」：滚动到原记录 */
  onJumpParent?: (task: Task) => void;
}

/**
 * 队列行：缩略图 + 提示词 + 模型 / 模式 / 参数 + 状态（带图标的 Tag）+ 进度 / 报错 + 操作。
 * ui-kit 的 FileRow 只有 32px 单行文字，放不下缩略图与多行信息，这里按它的样式约定自写一行。
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

  return (
    <li
      id={`task-row-${task.id}`}
      className={cn(
        "flex items-start gap-3 border-b px-3 py-2.5 transition-colors duration-slow last:border-b-0",
        failed && FILE_ROW_ERROR_CLASS,
        highlighted && "bg-primary-bg"
      )}
    >
      <Thumb
        path={cover}
        size="md"
        loading={task.status === "generating" || task.status === "downloading"}
        open={previewOpen}
        onOpenChange={setPreviewOpen}
        alt={task.prompt}
      />

      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <div className="flex min-w-0 items-baseline gap-2">
          <span className={cn("shrink-0 font-strong tabular-nums", failed ? "text-inherit" : "text-primary-text")}>#{task.number}</span>
          <Typography.Text strong ellipsis={{ tooltip: task.prompt }} className={cn("min-w-0", failed && "text-inherit")}>
            {task.prompt}
          </Typography.Text>
        </div>
        <div className="flex flex-wrap items-center gap-x-1 gap-y-1">
          <StatusTag task={task} />
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
          <span className={cn("text-sm", failed ? "text-inherit" : "text-text-tertiary")}>
            {paramsSummary(task.model, task.params)} · {formatTime(task.createdAt)}
          </span>
        </div>
        {task.status === "generating" && task.remoteId && (
          <Progress percent={task.progress} size="small" showInfo={false} status="active" className="m-0 max-w-[320px]" />
        )}
        {task.error && <span className="text-sm">{task.error}</span>}
        {task.notice && <span className="text-sm text-warning-text">{task.notice}</span>}
        {parts.length > 0 && (
          <Image.PreviewGroup>
            <div className="flex gap-1.5">
              {parts.map((p, i) => (
                <Tooltip key={p} title={`#${task.number}-${i + 1}`}>
                  <span className="flex flex-col items-center gap-0.5">
                    <Thumb path={p} size="sm" alt={`#${task.number}-${i + 1}`} />
                    <span className="text-sm tabular-nums text-text-tertiary">
                      #{task.number}-{i + 1}
                    </span>
                  </span>
                </Tooltip>
              ))}
            </div>
          </Image.PreviewGroup>
        )}
      </div>

      <div className="flex shrink-0 items-center">
        {done && file && (
          <Tooltip title="预览">
            <Button type="text" size="small" icon={<EyeOutlined />} aria-label="预览" onClick={() => setPreviewOpen(true)} />
          </Tooltip>
        )}
        {done && file && parts.length === 0 && (
          <Tooltip title="继续修改">
            <Button type="text" size="small" icon={<EditOutlined />} aria-label="继续修改" onClick={() => onEdit(task)} />
          </Tooltip>
        )}
        {done && parts.length > 0 && (
          <Dropdown
            trigger={["click"]}
            menu={{
              items: parts.map((p, i) => ({ key: p, label: `继续修改 #${task.number}-${i + 1}` })),
              onClick: ({ key }) => onEdit(task, key),
            }}
          >
            <Tooltip title="继续修改（选一张）">
              <Button type="text" size="small" icon={<EditOutlined />} aria-label="继续修改" />
            </Tooltip>
          </Dropdown>
        )}
        {task.status === "download_failed" && (
          <Tooltip title="仅重试下载（不重新生成，不扣费）">
            <Button type="text" size="small" icon={<CloudDownloadOutlined />} aria-label="重试下载" onClick={() => onRetryDownload(task)} />
          </Tooltip>
        )}
        {(failed || task.status === "cancelled") && (
          <Tooltip title={task.status === "download_failed" ? "重新生成（会再次扣费）" : "重试"}>
            <Button type="text" size="small" icon={<RedoOutlined />} aria-label="重试" onClick={() => onRetry(task)} />
          </Tooltip>
        )}
        {done && file && (
          <Tooltip title="打开所在文件夹">
            <Button type="text" size="small" icon={<FolderOpenOutlined />} aria-label="打开所在文件夹" onClick={() => onReveal(task)} />
          </Tooltip>
        )}
        {active && (
          <Tooltip title={task.status === "queued" ? "取消排队" : "取消（丢弃结果）"}>
            <Button type="text" size="small" icon={<StopOutlined />} aria-label="取消" onClick={() => onCancel(task)} />
          </Tooltip>
        )}
        <Tooltip title="删除记录…">
          <Button type="text" size="small" danger icon={<DeleteOutlined />} aria-label="删除记录" onClick={() => onDelete(task)} />
        </Tooltip>
      </div>
    </li>
  );
}

/** 「改自 #7」/「改自 #7-2」：可点击跳到原记录；原记录已删除时置灰 */
function ParentTag({ task, parent, onJump }: { task: Task; parent?: Task; onJump?: (task: Task) => void }) {
  const number = parent?.number ?? task.parentNumber;
  const label = number ? `改自 #${number}${task.parentPart ? `-${task.parentPart}` : ""}` : "改自已删除的记录";
  if (!parent) {
    return (
      <Tooltip title="原记录已删除">
        <Tag icon={<BranchesOutlined />} variant="filled" className="text-text-tertiary">
          {label}
        </Tag>
      </Tooltip>
    );
  }
  return (
    <Tooltip title={`${parent.inQueue ? "定位到" : "在图库中查看"}原记录：${shortPrompt(parent.prompt, 24)}`}>
      <Tag
        icon={<BranchesOutlined />}
        variant="outlined"
        role="button"
        tabIndex={0}
        className="cursor-pointer transition-colors duration-fast hover:border-primary hover:text-primary-text"
        onClick={() => onJump?.(task)}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onJump?.(task);
          }
        }}
      >
        {label}
      </Tag>
    </Tooltip>
  );
}
