import brandMark from "../assets/brand-mark.png";

/**
 * 品牌标记：直接用应用图标（src-tauri/app-icon.svg 裁到底板、去掉阴影，四角透明），
 * 各主题、深浅色下都显示同一张图。
 *
 * AppShell 把 brand.mark 放在 40px 的反色块里（为单色字形设计），这里把图标画成 42px、
 * 略大于色块并居中，正好盖住色块与圆角，不改 ui-kit 也能显示原色图标。
 */
export function BrandMark({ size = "shell" }: { size?: "shell" | "md" }) {
  return (
    <img
      src={brandMark}
      alt=""
      aria-hidden
      draggable={false}
      className={size === "shell" ? "size-[42px] max-w-none select-none" : "size-10 select-none"}
    />
  );
}
