import { useState } from "react";
import { FileImageOutlined, LoadingOutlined } from "@ant-design/icons";
import { Image } from "antd";
import { convertFileSrc } from "@tauri-apps/api/core";
import { cn } from "../ui-kit";

interface ThumbProps {
  /** 本地文件路径；没有时显示占位图标 */
  path?: string;
  size: "sm" | "md" | "lg";
  loading?: boolean;
  /** 点击缩略图可大图预览（antd Image 预览） */
  preview?: boolean;
  /** 受控打开预览（例如「预览」按钮） */
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  alt?: string;
  className?: string;
}

const BOX = { sm: "size-10", md: "size-16", lg: "h-[268px] w-full" } as const;

/** 本地图片缩略图：convertFileSrc + antd Image；固定方框、居中裁切。 */
export function Thumb({ path, size, loading, preview = true, open, onOpenChange, alt = "", className }: ThumbProps) {
  const [failed, setFailed] = useState(false);
  const box = cn("shrink-0 overflow-hidden rounded border bg-fill-alter", BOX[size], className);
  if (!path || failed) {
    return (
      <span className={cn(box, "grid place-items-center text-text-tertiary", size === "lg" ? "text-h2" : "text-xl")}>
        {loading ? <LoadingOutlined /> : <FileImageOutlined />}
      </span>
    );
  }
  const controlled = open !== undefined;
  return (
    <span className={box}>
      <Image
        src={convertFileSrc(path)}
        alt={alt}
        classNames={{ root: "block size-full", image: cn("size-full", size === "lg" ? "object-contain" : "object-cover") }}
        onError={() => setFailed(true)}
        preview={preview ? (controlled ? { open, onOpenChange } : true) : false}
      />
    </span>
  );
}
