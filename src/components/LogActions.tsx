import { ClearOutlined, CopyOutlined } from "@ant-design/icons";
import { App as AntApp, Button, Tooltip } from "antd";
import { logsToText, type LogEntry } from "../ui-kit";

/**
 * 日志面板标题栏右侧的「复制全部」「清空」。
 * kit 的 LogViewer 传 onClear 时把这两个按钮浮在日志区右上角，会盖住长日志行的末尾，
 * 又没有放到 Panel extra 的选项，所以这里不传 onClear，按钮由页面放在 Panel 的 extra。
 */
export function LogActions({ logs, onClear }: { logs: LogEntry[]; onClear: () => void }) {
  const { message } = AntApp.useApp();
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(logsToText(logs));
      message.success("已复制全部日志");
    } catch (err) {
      message.error(`复制失败：${String(err)}`);
    }
  };
  return (
    <span className="flex items-center">
      <Tooltip title="复制全部">
        <Button type="text" size="small" icon={<CopyOutlined />} aria-label="复制全部日志" disabled={!logs.length} onClick={copy} />
      </Tooltip>
      <Tooltip title="清空">
        <Button type="text" size="small" icon={<ClearOutlined />} aria-label="清空日志" disabled={!logs.length} onClick={onClear} />
      </Tooltip>
    </span>
  );
}
