import { RatioIcon } from "../ui-kit";
import { GPT_RATIOS, NANO_RATIOS } from "../lib/options";

/** 比例下拉项：kit 的 RatioIcon（按真实宽高比画的小矩形）+ 比例 + 尺寸 */
function ratioLabel(value: string, size?: string) {
  return (
    <span className="flex items-center gap-2">
      <RatioIcon ratio={value} className="text-text-secondary" />
      <span>{value === "auto" ? "自动" : value}</span>
      {size && value !== "auto" && <span className="text-text-tertiary">{size}</span>}
    </span>
  );
}

export const gptRatioOptions = GPT_RATIOS.map((r) => ({ value: r.value, label: ratioLabel(r.value, r.size) }));
export const nanoRatioOptions = NANO_RATIOS.map((r) => ({ value: r, label: ratioLabel(r) }));
