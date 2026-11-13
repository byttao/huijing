import { GPT_RATIOS, NANO_RATIOS, parseRatio, sizeText } from "../lib/options";
import type { GridPlan } from "../lib/types";

/** 比例小图示：按真实宽高比画的矩形（长边固定），用在下拉项里。auto 画虚线方框。 */
export function RatioIcon({ ratio }: { ratio: string }) {
  const r = parseRatio(ratio);
  const box = 14;
  const [w, h] = r ? (r[0] >= r[1] ? [box, (box * r[1]) / r[0]] : [(box * r[0]) / r[1], box]) : [box, box];
  return (
    <svg viewBox="0 0 16 16" className="size-4 shrink-0 text-text-secondary" aria-hidden>
      <rect
        x={(16 - w) / 2}
        y={(16 - h) / 2}
        width={Math.max(w, 1.2)}
        height={Math.max(h, 1.2)}
        rx={1}
        fill="none"
        stroke="currentColor"
        strokeWidth={1.2}
        strokeDasharray={r ? undefined : "2 1.5"}
      />
    </svg>
  );
}

function ratioLabel(value: string, size?: string) {
  return (
    <span className="flex items-center gap-2">
      <RatioIcon ratio={value} />
      <span>{value === "auto" ? "自动" : value}</span>
      {size && value !== "auto" && <span className="text-text-tertiary">{size}</span>}
    </span>
  );
}

export const gptRatioOptions = GPT_RATIOS.map((r) => ({ value: r.value, label: ratioLabel(r.value, r.size) }));
export const nanoRatioOptions = NANO_RATIOS.map((r) => ({ value: r, label: ratioLabel(r) }));

interface GridPreviewProps {
  plan: GridPlan | null;
  /** 无法规划时的原因（例如比例为自动时不能多合一） */
  error?: string | null;
}

/**
 * 当前比例的预览框：单张画出比例；多合一画出合成比例、宫格划分（几列几行），
 * 合成比例不精确时用实线框标出每格居中裁切的范围；下方写请求尺寸与拆切后每张的尺寸。
 */
export function GridPreview({ plan, error }: GridPreviewProps) {
  if (!plan) return error ? <span className="text-sm text-text-tertiary">{error}</span> : null;
  const multi = plan.count > 1;
  const cell = parseRatio(plan.cellRatio);
  const req = parseRatio(plan.requestRatio) ?? cell;
  if (!req) {
    return <span className="text-sm text-text-tertiary">比例为「自动」：由模型决定画面比例，尺寸出图后才知道。</span>;
  }
  // viewBox 用合成比例；preserveAspectRatio 默认 meet，框在 SVG 里按真实比例显示
  const W = 100;
  const H = (100 * req[1]) / req[0];
  const cw = W / plan.cols;
  const ch = H / plan.rows;
  const stroke = W / 120;
  const cells = Array.from({ length: plan.cols * plan.rows }, (_, i) => ({ c: i % plan.cols, r: Math.floor(i / plan.cols), i }));
  const crop = (c: number, r: number) => {
    if (!cell || plan.exact) return null;
    const target = cell[0] / cell[1];
    const [fw, fh] = cw / ch > target ? [ch * target, ch] : [cw, cw / target];
    return { x: c * cw + (cw - fw) / 2, y: r * ch + (ch - fh) / 2, w: fw, h: fh };
  };
  const approx = plan.estimated ? "约 " : "";
  const tier = plan.requestTier ? ` · ${plan.requestTier}` : "";

  return (
    <div className="flex flex-col gap-1.5">
      <div className="grid place-items-center rounded border bg-fill-alter p-2">
        <svg viewBox={`${-stroke} ${-stroke} ${W + 2 * stroke} ${H + 2 * stroke}`} className="h-[110px] w-full" role="img" aria-label={`预览：${plan.cols} 列 ${plan.rows} 行，合成比例 ${plan.requestRatio}`}>
          <rect x={0} y={0} width={W} height={H} className="fill-primary-bg stroke-primary" strokeWidth={stroke} />
          {multi &&
            cells.map(({ c, r, i }) => {
              const box = crop(c, r);
              return (
                <g key={i}>
                  {box && <rect x={box.x} y={box.y} width={box.w} height={box.h} className="fill-container stroke-primary" strokeWidth={stroke * 0.7} />}
                  <text
                    x={c * cw + cw / 2}
                    y={r * ch + ch / 2}
                    textAnchor="middle"
                    dominantBaseline="central"
                    fontSize={Math.min(cw, ch) * 0.32}
                    className="fill-primary-text"
                  >
                    {i + 1}
                  </text>
                </g>
              );
            })}
          {multi &&
            Array.from({ length: plan.cols - 1 }, (_, k) => (
              <line key={`v${k}`} x1={(k + 1) * cw} y1={0} x2={(k + 1) * cw} y2={H} className="stroke-primary" strokeWidth={stroke} strokeDasharray={`${stroke * 3} ${stroke * 2}`} />
            ))}
          {multi &&
            Array.from({ length: plan.rows - 1 }, (_, k) => (
              <line key={`h${k}`} x1={0} y1={(k + 1) * ch} x2={W} y2={(k + 1) * ch} className="stroke-primary" strokeWidth={stroke} strokeDasharray={`${stroke * 3} ${stroke * 2}`} />
            ))}
        </svg>
      </div>
      <div className="flex flex-col text-sm text-text-tertiary">
        {multi ? (
          <>
            <span>
              {plan.cols} 列 × {plan.rows} 行 · 合成比例 {plan.requestRatio}
              {tier} · 请求 {approx}
              {sizeText(plan.requestSize)}
            </span>
            <span>
              拆切后每张 {approx}
              {sizeText(plan.cellSize)}
              {!plan.exact && `（合成比例取最接近的 ${plan.requestRatio}，每格居中裁到 ${plan.cellRatio}）`}
              {plan.upscale ? ` → 缩放到单张原尺寸 ${approx}${sizeText(plan.outputSize)}` : "（不放大）"}
            </span>
          </>
        ) : (
          <span>
            单张 · {plan.cellRatio}
            {tier} · {approx}
            {sizeText(plan.singleSize)}
          </span>
        )}
      </div>
    </div>
  );
}
