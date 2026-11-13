import { CloseOutlined } from "@ant-design/icons";
import { Button, Tooltip } from "antd";
import { fileName } from "../ui-kit";
import { Thumb } from "./Thumb";

/** 参考图缩略图网格：每张可预览、可移除。 */
export function RefImages({ paths, onRemove }: { paths: string[]; onRemove?: (path: string) => void }) {
  if (!paths.length) return null;
  return (
    <ul className="m-0 grid list-none grid-cols-4 gap-2 p-0">
      {paths.map((path) => (
        <li key={path} className="relative">
          <Tooltip title={fileName(path)}>
            <span className="block">
              <Thumb path={path} size="md" className="aspect-square h-auto w-full" alt={fileName(path)} />
            </span>
          </Tooltip>
          {onRemove && (
            <Button
              size="small"
              shape="circle"
              icon={<CloseOutlined />}
              aria-label={`移除 ${fileName(path)}`}
              className="absolute -right-1.5 -top-1.5 size-5 min-w-0"
              onClick={() => onRemove(path)}
            />
          )}
        </li>
      ))}
    </ul>
  );
}
