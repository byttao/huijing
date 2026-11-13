import {
  CheckCircleOutlined,
  ClockCircleOutlined,
  CloseCircleOutlined,
  ExclamationCircleOutlined,
  MinusCircleOutlined,
  SyncOutlined,
} from "@ant-design/icons";
import { Tag } from "antd";
import type { Task } from "../lib/types";

/** 任务状态标签：一律带图标，不单靠颜色（森翠主题与成功色同色系）。 */
export function StatusTag({ task }: { task: Task }) {
  switch (task.status) {
    case "queued":
      return <Tag icon={<ClockCircleOutlined />} color="default">排队中</Tag>;
    case "generating":
      return (
        <Tag icon={<SyncOutlined spin />} color="processing">
          {task.remoteId ? `生成中 ${task.progress}%` : "生成中"}
        </Tag>
      );
    case "downloading":
      return <Tag icon={<SyncOutlined spin />} color="processing">下载中</Tag>;
    case "succeeded":
      return <Tag icon={<CheckCircleOutlined />} color="success">已完成</Tag>;
    case "failed":
      return <Tag icon={<CloseCircleOutlined />} color="error">失败</Tag>;
    case "download_failed":
      return <Tag icon={<ExclamationCircleOutlined />} color="warning">下载失败</Tag>;
    case "cancelled":
      return <Tag icon={<MinusCircleOutlined />} color="default">已取消</Tag>;
  }
}
